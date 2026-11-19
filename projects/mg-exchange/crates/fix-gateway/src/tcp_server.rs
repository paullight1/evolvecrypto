//! FIX 4.4 TCP server — accepts connections, manages sessions, routes messages.
//!
//! # Design
//!
//! The server is generic over an [`ExchangeAdapter`] trait so it can be tested
//! without pulling in the full exchange stack. In production, wrap
//! `Arc<Exchange>` in a newtype that implements `ExchangeAdapter`.
//!
//! The testable entry point is [`route_message`], which processes a single
//! parsed FIX message and returns zero or more response messages. TCP I/O is
//! handled by [`FixTcpServer::start`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use exchange_common::{CancelOrderCommand, MatchEvent, PlaceOrderCommand, UserId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::builder::{build_execution_report, build_heartbeat, build_logout};
use crate::parser::{msg_type, tag, FixMessage};
use crate::session::{FixSession, SessionAction};
use crate::translator::{fix_to_cancel_order, fix_to_place_order, order_accepted_to_report, order_rejected_to_report};

// ---------------------------------------------------------------------------
// Exchange adapter trait
// ---------------------------------------------------------------------------

/// Minimal exchange interface required by the FIX server.
///
/// Implement this trait for `Arc<Exchange>` (or any mock) to decouple the
/// FIX gateway from the full exchange stack.
pub trait ExchangeAdapter: Send + Sync + 'static {
    /// Place a new order. Returns the list of match events produced.
    fn place_order(&self, cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>>;

    /// Cancel an existing order. Returns the list of events produced.
    fn cancel_order(&self, cmd: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>>;

    /// Authenticate a FIX client using API key credentials from the Logon message.
    ///
    /// `api_key` is extracted from FIX tag 553 (Username).
    /// `api_secret` is extracted from FIX tag 554 (Password).
    ///
    /// Returns the authenticated UserId on success, or an error message on failure.
    fn authenticate_fix(&self, api_key: &str, api_secret: &str) -> anyhow::Result<UserId> {
        // Default: no authentication configured — reject all connections.
        let _ = (api_key, api_secret);
        Err(anyhow::anyhow!("FIX authentication not configured"))
    }

    /// Whether FIX authentication is required (true in production).
    ///
    /// When false, connections are accepted with a random UserId (dev/test only).
    fn fix_auth_required(&self) -> bool {
        true
    }

    /// Subscribe to unsolicited ExecutionReport broadcasts (drop copy).
    ///
    /// Returns a receiver that yields serialized FIX ExecutionReport bytes for
    /// every fill in the exchange. Returns `None` if drop copy is not supported.
    fn subscribe_executions(&self) -> Option<tokio::sync::broadcast::Receiver<Vec<u8>>> {
        None
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the FIX TCP server.
#[derive(Debug, Clone)]
pub struct FixTcpServerConfig {
    /// Address to bind, e.g. `"0.0.0.0:8081"`.
    pub bind_addr: String,
    /// Heartbeat interval in seconds negotiated with clients.
    pub heartbeat_interval: u32,
    /// The exchange's CompID, e.g. `"EXCHANGE"`.
    pub target_comp_id: String,
}

impl Default for FixTcpServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8081".to_string(),
            heartbeat_interval: 30,
            target_comp_id: "EXCHANGE".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// FIX 4.4 TCP server.
///
/// Spawns a tokio task per accepted TCP connection. Each connection runs its
/// own [`FixSession`]. In production, call [`FixTcpServer::start`].
/// For unit testing, use [`route_message`] directly to exercise routing logic
/// without a TCP connection.
pub struct FixTcpServer {
    addr: SocketAddr,
    config: FixTcpServerConfig,
}

impl FixTcpServer {
    /// Create a new server from config. Returns an error if the bind address
    /// cannot be parsed.
    pub fn new(config: FixTcpServerConfig) -> Result<Self> {
        let addr: SocketAddr = config.bind_addr.parse()
            .map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", config.bind_addr, e))?;
        Ok(Self { addr, config })
    }

    /// P3-2: Check TLS configuration and warn about unencrypted connections.
    ///
    /// This method documents the current state of FIX TLS support:
    ///
    /// **Phase 1 (current):** TLS is NOT implemented in the TCP acceptor.
    /// All FIX connections are plain TCP. In production, deploy behind a
    /// TLS-terminating proxy (nginx/haproxy) to satisfy the encryption requirement.
    ///
    /// **Phase 2 (TODO):** Replace `TcpStream` with `tokio_rustls::TlsAcceptor`
    /// wrapping the stream after `listener.accept()`. Load cert/key via
    /// `rustls::ServerConfig::builder()` from `FIX_TLS_CERT`/`FIX_TLS_KEY` paths.
    ///
    /// # Behavior
    ///
    /// - `PRODUCTION=true` + `FIX_TLS_CERT`/`FIX_TLS_KEY` set:
    ///   logs an ERROR because the certs are configured but TLS is not yet wired
    ///   in. Connections are UNENCRYPTED. Deploy behind a TLS-terminating proxy.
    /// - `PRODUCTION=true` + `FIX_REQUIRE_TLS=true`:
    ///   returns `Err` — refuses to start the FIX gateway entirely.
    /// - Non-production: logs a warning (plaintext is acceptable for dev/test).
    pub fn check_tls_requirement() -> Result<()> {
        let is_production = std::env::var("PRODUCTION").unwrap_or_default() == "true";
        let require_tls = std::env::var("FIX_REQUIRE_TLS").unwrap_or_default() == "true";

        if is_production {
            let has_cert = std::env::var("FIX_TLS_CERT").is_ok();
            let has_key = std::env::var("FIX_TLS_KEY").is_ok();

            // Strict mode: refuse to start if TLS is not implemented.
            if require_tls {
                return Err(anyhow::anyhow!(
                    "FIX_REQUIRE_TLS=true but native TLS is not yet implemented. \
                    Deploy behind a TLS-terminating proxy (nginx/haproxy) and unset \
                    FIX_REQUIRE_TLS, or wait for Phase 2 (tokio-rustls integration)."
                ));
            }

            if has_cert || has_key {
                // Certs are set but TLS is not wired into the acceptor yet.
                tracing::error!(
                    "FIX TLS is required in production but not yet implemented — \
                    FIX connections are UNENCRYPTED. \
                    Deploy behind a TLS-terminating proxy (nginx/haproxy). \
                    Set FIX_REQUIRE_TLS=true to prevent startup if unencrypted FIX is unacceptable."
                );
            } else {
                // No certs configured — assume proxy-terminated TLS, but still warn.
                tracing::error!(
                    "FIX TLS is required in production but not yet implemented — \
                    FIX connections are UNENCRYPTED. \
                    Deploy behind a TLS-terminating proxy (nginx/haproxy). \
                    Set FIX_REQUIRE_TLS=true to prevent startup if unencrypted FIX is unacceptable."
                );
            }
        } else {
            tracing::warn!("FIX TLS: running in plaintext mode (non-production)");
        }
        Ok(())
    }

    /// Start accepting TCP connections.
    ///
    /// For each accepted connection a new tokio task is spawned to handle the
    /// FIX session lifecycle: logon → message exchange → logout/disconnect.
    pub async fn start<E: ExchangeAdapter>(self, exchange: Arc<E>) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await
            .map_err(|e| anyhow::anyhow!("Failed to bind {}: {}", self.addr, e))?;

        tracing::info!(addr = %self.addr, "FIX TCP server listening");

        // M25-3: Per-IP connection tracking. Max 5 FIX sessions per source IP.
        let ip_connections: Arc<Mutex<HashMap<std::net::IpAddr, u32>>> =
            Arc::new(Mutex::new(HashMap::new()));
        const MAX_FIX_CONNECTIONS_PER_IP: u32 = 5;

        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let peer_ip = peer.ip();

                    // Check per-IP connection limit.
                    {
                        let mut counts = ip_connections.lock().await;
                        let count = counts.entry(peer_ip).or_insert(0);
                        if *count >= MAX_FIX_CONNECTIONS_PER_IP {
                            tracing::warn!(
                                peer = %peer,
                                count = *count,
                                "FIX connection rejected: per-IP limit exceeded"
                            );
                            // Drop the connection — no FIX session established.
                            drop(stream);
                            continue;
                        }
                        *count += 1;
                    }

                    tracing::info!(peer = %peer, "FIX connection accepted");
                    let cfg = self.config.clone();
                    let exc = Arc::clone(&exchange);
                    let ip_conns = Arc::clone(&ip_connections);
                    tokio::spawn(async move {
                        let result = handle_connection(stream, peer, cfg, exc).await;
                        if let Err(e) = &result {
                            tracing::warn!(peer = %peer, error = %e, "FIX connection error");
                        }
                        // Decrement connection count on disconnect.
                        let mut counts = ip_conns.lock().await;
                        if let Some(count) = counts.get_mut(&peer_ip) {
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                counts.remove(&peer_ip);
                            }
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(error = %e, "FIX accept error");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

/// Handle a single TCP connection through its full lifecycle.
#[allow(unused_assignments)]
async fn handle_connection<E: ExchangeAdapter>(
    stream: TcpStream,
    peer: SocketAddr,
    config: FixTcpServerConfig,
    exchange: Arc<E>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let mut lines = BufReader::new(reader).lines();

    let auth_required = exchange.fix_auth_required();

    // ── Phase 1: Wait for Logon and authenticate ────────────────────────
    //
    // The first message MUST be a Logon. We extract API credentials from
    // FIX tags 553 (Username = API key) and 554 (Password = API secret).
    // If auth is required and credentials are missing or invalid, we send
    // a Logout with reason and close the connection.

    let mut user_id: UserId = Uuid::nil();
    let mut session: Option<FixSession> = None;
    let mut _authenticated = false;

    // Read the first message — must be Logon.
    let first_line = match lines.next_line().await? {
        Some(l) => l,
        None => return Ok(()),
    };

    let first_msg = match FixMessage::parse(first_line.as_bytes()) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(peer = %peer, error = %e, "FIX: failed to parse first message");
            return Ok(());
        }
    };

    if first_msg.msg_type() != Some(msg_type::LOGON) {
        tracing::warn!(peer = %peer, "FIX: first message is not Logon — disconnecting");
        return Ok(());
    }

    let sender = match first_msg.get_field(tag::SENDER_COMP_ID) {
        Some(s) => s.to_owned(),
        None => {
            tracing::warn!(peer = %peer, "FIX Logon missing SenderCompID");
            return Ok(());
        }
    };

    session = Some(FixSession::new(&config.target_comp_id, &sender, config.heartbeat_interval));

    if auth_required {
        let api_key = first_msg.get_field(tag::USERNAME).unwrap_or("");
        let api_secret = first_msg.get_field(tag::PASSWORD).unwrap_or("");

        if api_key.is_empty() {
            tracing::warn!(peer = %peer, "FIX Logon missing credentials (tag 553/554)");
            let logout = build_logout(&config.target_comp_id, &sender, "Authentication failed: missing credentials");
            let mut w = writer.lock().await;
            w.write_all(&logout.to_bytes()).await?;
            return Ok(());
        }

        match exchange.authenticate_fix(api_key, api_secret) {
            Ok(uid) => {
                user_id = uid;
                _authenticated = true;
                tracing::info!(
                    peer = %peer,
                    user_id = %uid,
                    "FIX Logon authenticated"
                );
            }
            Err(e) => {
                tracing::warn!(peer = %peer, error = %e, "FIX authentication failed");
                let logout = build_logout(&config.target_comp_id, &sender, "Authentication failed");
                let mut w = writer.lock().await;
                w.write_all(&logout.to_bytes()).await?;
                return Ok(());
            }
        }
    } else {
        // Dev mode: accept any connection with a random UserId.
        user_id = Uuid::new_v4();
        _authenticated = true;
        tracing::warn!(
            peer = %peer,
            user_id = %user_id,
            "FIX Logon accepted WITHOUT authentication (dev mode)"
        );
    }

    // Complete the logon handshake.
    let sess = session.as_mut().expect("session initialised above");
    let logon_responses = route_message(&first_msg, sess, &exchange, user_id);
    {
        let mut w = writer.lock().await;
        for resp in logon_responses {
            w.write_all(&resp.to_bytes()).await?;
        }
    }

    // ── Phase 2: Message loop ───────────────────────────────────────────

    // Spawn drop-copy writer if the exchange supports it.
    let drop_copy_handle = if let Some(mut rx) = exchange.subscribe_executions() {
        let w = Arc::clone(&writer);
        Some(tokio::spawn(async move {
            while let Ok(report_bytes) = rx.recv().await {
                let mut guard = w.lock().await;
                if guard.write_all(&report_bytes).await.is_err() {
                    break; // Client disconnected
                }
            }
        }))
    } else {
        None
    };

    loop {
        let line = match lines.next_line().await? {
            Some(l) => l,
            None => break,
        };

        let raw = line.as_bytes();
        let msg = match FixMessage::parse(raw) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(peer = %peer, error = %e, "Failed to parse FIX message");
                continue;
            }
        };

        let responses = route_message(&msg, sess, &exchange, user_id);

        {
            let mut w = writer.lock().await;
            for resp in responses {
                let bytes = resp.to_bytes();
                w.write_all(&bytes).await?;
            }
        }

        // If the session was disconnected, close the connection.
        if !sess.is_logged_on && msg.msg_type() == Some(msg_type::LOGOUT) {
            tracing::info!(peer = %peer, "FIX session logged out — closing connection");
            break;
        }
    }

    // Clean up drop-copy writer task.
    if let Some(handle) = drop_copy_handle {
        handle.abort();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Message router (testable without TCP)
// ---------------------------------------------------------------------------

/// Route a parsed FIX message through the exchange.
///
/// This is the core logic of the FIX server and is fully testable without a
/// TCP connection. It processes session-level actions (logon, heartbeat,
/// logout) and dispatches application messages (NewOrderSingle,
/// OrderCancelRequest) to the exchange adapter.
///
/// Returns the list of FIX messages to send back to the client.
pub fn route_message<E: ExchangeAdapter>(
    msg: &FixMessage,
    session: &mut FixSession,
    exchange: &Arc<E>,
    user_id: UserId,
) -> Vec<FixMessage> {
    let action = session.on_message_received(msg.clone());

    match action {
        SessionAction::SendLogon => {
            tracing::info!(
                client = %session.target_comp_id,
                "FIX Logon — session established"
            );
            vec![session.make_logon()]
        }

        SessionAction::SendHeartbeat => {
            vec![session.make_heartbeat()]
        }

        SessionAction::SendReject { reason } => {
            tracing::warn!(reason = %reason, "FIX session-level reject");
            // Build a minimal session-level Reject (MsgType=3).
            // We send a heartbeat as a safe fallback for Phase 1.
            vec![build_heartbeat(&session.sender_comp_id, &session.target_comp_id)]
        }

        SessionAction::SendResendRequest { begin_seq, end_seq } => {
            // M25-2: Request the counterparty to resend missed messages.
            tracing::info!(begin_seq, end_seq, "FIX ResendRequest for gap recovery");
            // Phase 1: log and send heartbeat as acknowledgement.
            // Full ResendRequest (MsgType=2) builder would be added in Phase 2.
            vec![build_heartbeat(&session.sender_comp_id, &session.target_comp_id)]
        }

        SessionAction::SendLogout { reason } => {
            tracing::info!(client = %session.target_comp_id, reason = %reason, "FIX Logout");
            vec![build_logout(&session.sender_comp_id, &session.target_comp_id, &reason)]
        }

        SessionAction::Disconnect => {
            tracing::info!(client = %session.target_comp_id, "FIX Disconnect — session closing");
            vec![]
        }

        SessionAction::Process(app_msg) => {
            route_application_message(&app_msg, session, exchange, user_id)
        }
    }
}

/// Route a validated application-level FIX message to the exchange.
fn route_application_message<E: ExchangeAdapter>(
    msg: &FixMessage,
    session: &mut FixSession,
    exchange: &Arc<E>,
    user_id: UserId,
) -> Vec<FixMessage> {
    let sender = &session.sender_comp_id;
    let target = &session.target_comp_id;

    match msg.msg_type() {
        Some(msg_type::NEW_ORDER_SINGLE) => {
            handle_new_order_single(msg, sender, target, exchange, user_id)
        }

        Some(msg_type::ORDER_CANCEL_REQUEST) => {
            handle_order_cancel_request(msg, sender, target, exchange, user_id)
        }

        Some(mt) => {
            tracing::debug!(msg_type = %mt, "FIX: unhandled application message type");
            vec![]
        }

        None => vec![],
    }
}

fn handle_new_order_single<E: ExchangeAdapter>(
    msg: &FixMessage,
    sender: &str,
    target: &str,
    exchange: &Arc<E>,
    user_id: UserId,
) -> Vec<FixMessage> {
    let cmd = match fix_to_place_order(msg, user_id) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "FIX: failed to translate NewOrderSingle");
            let dummy_id = Uuid::new_v4();
            return vec![order_rejected_to_report(dummy_id, &e.to_string(), sender, target)];
        }
    };

    let order_id = cmd.order_id;
    let symbol = cmd.symbol.as_str().to_owned();

    match exchange.place_order(cmd) {
        Ok(events) => {
            // Check for rejection first
            if let Some(MatchEvent::OrderRejected { order_id, reason, .. }) =
                events.iter().find(|e| matches!(e, MatchEvent::OrderRejected { .. }))
            {
                tracing::info!(order_id = %order_id, reason = %reason, "FIX: order rejected");
                return vec![order_rejected_to_report(*order_id, reason, sender, target)];
            }

            // Build execution reports for all fills
            let mut reports: Vec<FixMessage> = Vec::new();

            // Send an initial New ack if the order was accepted
            if events.iter().any(|e| matches!(e, MatchEvent::OrderAccepted { .. })) {
                reports.push(order_accepted_to_report(order_id, &symbol, sender, target));
            }

            // Add a Trade report for each fill
            for event in &events {
                if let MatchEvent::Fill(fill) = event {
                    let side_str = if fill.taker_order_id == order_id { "1" } else { "2" };
                    let exec_id = fill.trade_id.to_string();
                    reports.push(build_execution_report(
                        sender,
                        target,
                        &order_id.to_string(),
                        &exec_id,
                        "F",       // ExecType: Trade
                        side_str,
                        fill.symbol.as_str(),
                        fill.qty,
                        fill.price,
                        "2",       // OrdStatus: Filled
                    ));
                }
            }

            // If no ack and no fills, send a New ack anyway (market order fully filled
            // path where OrderAccepted is not emitted by the engine).
            if reports.is_empty() {
                reports.push(order_accepted_to_report(order_id, &symbol, sender, target));
            }

            reports
        }
        Err(e) => {
            tracing::warn!(order_id = %order_id, error = %e, "FIX: exchange rejected order");
            vec![order_rejected_to_report(order_id, &e.to_string(), sender, target)]
        }
    }
}

fn handle_order_cancel_request<E: ExchangeAdapter>(
    msg: &FixMessage,
    sender: &str,
    target: &str,
    exchange: &Arc<E>,
    user_id: UserId,
) -> Vec<FixMessage> {
    let cmd = match fix_to_cancel_order(msg, user_id) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "FIX: failed to translate OrderCancelRequest");
            // OrderCancelReject (MsgType=9) — return a heartbeat as fallback
            return vec![build_heartbeat(sender, target)];
        }
    };

    let order_id = cmd.order_id;
    let symbol_str = cmd.symbol.as_str().to_owned();

    match exchange.cancel_order(cmd) {
        Ok(_) => {
            tracing::info!(order_id = %order_id, "FIX: order cancelled");
            // ExecType=4 (Canceled), OrdStatus=4 (Canceled)
            vec![build_execution_report(
                sender,
                target,
                &order_id.to_string(),
                &Uuid::new_v4().to_string(),
                "4",   // ExecType: Canceled
                "0",   // Side placeholder (cancel doesn't change side)
                &symbol_str,
                rust_decimal::Decimal::ZERO,
                rust_decimal::Decimal::ZERO,
                "4",   // OrdStatus: Canceled
            )]
        }
        Err(e) => {
            tracing::warn!(order_id = %order_id, error = %e, "FIX: cancel failed");
            // Return a heartbeat as a session-safe fallback for Phase 1.
            vec![build_heartbeat(sender, target)]
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::FixMessageBuilder;
    use crate::parser::{tag, FixMessage};
    use std::sync::Mutex;
    use uuid::Uuid;

    // --- Mock exchange adapter ---

    struct MockExchange {
        /// Orders placed so far (order_id, symbol, side, qty, price).
        placed: Mutex<Vec<PlaceOrderCommand>>,
        /// Order IDs cancelled so far.
        cancelled: Mutex<Vec<Uuid>>,
        /// If set, place_order returns this error.
        place_error: Option<String>,
        /// If set, cancel_order returns this error.
        cancel_error: Option<String>,
    }

    impl MockExchange {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                placed: Mutex::new(vec![]),
                cancelled: Mutex::new(vec![]),
                place_error: None,
                cancel_error: None,
            })
        }

        fn with_place_error(msg: &str) -> Arc<Self> {
            Arc::new(Self {
                placed: Mutex::new(vec![]),
                cancelled: Mutex::new(vec![]),
                place_error: Some(msg.to_string()),
                cancel_error: None,
            })
        }

        #[allow(dead_code)]
        fn with_cancel_error(msg: &str) -> Arc<Self> {
            Arc::new(Self {
                placed: Mutex::new(vec![]),
                cancelled: Mutex::new(vec![]),
                place_error: None,
                cancel_error: Some(msg.to_string()),
            })
        }
    }

    impl ExchangeAdapter for MockExchange {
        fn place_order(&self, cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
            if let Some(ref e) = self.place_error {
                return Err(anyhow::anyhow!("{e}"));
            }
            self.placed.lock().unwrap().push(cmd.clone());
            // Return an accepted event so callers see the order was acknowledged.
            Ok(vec![MatchEvent::OrderAccepted {
                order_id: cmd.order_id,
                symbol: cmd.symbol.clone(),
                seq: 0,
            }])
        }

        fn cancel_order(&self, cmd: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
            if let Some(ref e) = self.cancel_error {
                return Err(anyhow::anyhow!("{e}"));
            }
            self.cancelled.lock().unwrap().push(cmd.order_id);
            Ok(vec![MatchEvent::OrderCancelled {
                order_id: cmd.order_id,
                symbol: cmd.symbol.clone(),
                remaining_qty: rust_decimal::Decimal::ZERO,
                seq: 0,
            }])
        }

        fn fix_auth_required(&self) -> bool {
            false // Tests run in dev mode
        }
    }

    // --- Helpers ---

    fn logged_on_session() -> FixSession {
        let mut s = FixSession::new("EXCHANGE", "CLIENT1", 30);
        s.is_logged_on = true;
        s.incoming_seq = 1;
        s
    }

    fn new_order_msg(seq: u64, symbol: &str, side: &str) -> FixMessage {
        let msg = FixMessageBuilder::new(crate::parser::msg_type::NEW_ORDER_SINGLE, "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, seq)
            .field(tag::CL_ORD_ID, Uuid::new_v4().to_string())
            .field(tag::SYMBOL, symbol)
            .field(tag::SIDE, side)          // "1" = Buy, "2" = Sell
            .field(tag::ORDER_QTY, "1.0")
            .field(tag::ORD_TYPE, "2")       // Limit
            .field(tag::PRICE, "50000.00")
            .build();
        FixMessage::parse(&msg.to_bytes()).expect("valid new order")
    }

    fn cancel_msg(seq: u64, order_id: Uuid, symbol: &str) -> FixMessage {
        let msg = FixMessageBuilder::new(crate::parser::msg_type::ORDER_CANCEL_REQUEST, "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, seq)
            .field(tag::CL_ORD_ID, Uuid::new_v4().to_string())
            .field(tag::ORIG_CL_ORD_ID, order_id.to_string())
            .field(tag::SYMBOL, symbol)
            .field(tag::SIDE, "1")
            .build();
        FixMessage::parse(&msg.to_bytes()).expect("valid cancel")
    }

    fn logon_msg(seq: u64) -> FixMessage {
        let msg = FixMessageBuilder::new(crate::parser::msg_type::LOGON, "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, seq)
            .field(tag::ENCRYPT_METHOD, "0")
            .field(tag::HEARTBEAT_INT, "30")
            .build();
        FixMessage::parse(&msg.to_bytes()).expect("valid logon")
    }

    // --- Tests ---

    /// test_fix_route_new_order — NewOrderSingle produces an ExecutionReport(New).
    #[test]
    fn test_fix_route_new_order() {
        let exchange = MockExchange::new();
        let mut session = logged_on_session();
        let user_id = Uuid::new_v4();

        let msg = new_order_msg(1, "BTCUSDT", "1");
        let responses = route_message(&msg, &mut session, &exchange, user_id);

        // Expect at least one ExecutionReport
        assert!(!responses.is_empty(), "should return at least one ExecutionReport");

        // The first response must be an ExecutionReport (MsgType=8) with ExecType=0 (New)
        let first = &responses[0];
        let bytes = first.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("parseable");
        assert_eq!(parsed.msg_type(), Some("8"), "must be ExecutionReport");
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("0"), "ExecType must be New");
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("0"), "OrdStatus must be New");

        // The mock recorded the order
        assert_eq!(exchange.placed.lock().unwrap().len(), 1);
    }

    /// test_fix_route_cancel — OrderCancelRequest produces an ExecutionReport(Cancelled).
    #[test]
    fn test_fix_route_cancel() {
        let exchange = MockExchange::new();
        let mut session = logged_on_session();
        let user_id = Uuid::new_v4();

        // Place first so we have something to cancel
        let place_msg = new_order_msg(1, "BTCUSDT", "1");
        let _ = route_message(&place_msg, &mut session, &exchange, user_id);

        // Get the order_id from what was placed
        let placed = exchange.placed.lock().unwrap();
        let order_id = placed[0].order_id;
        drop(placed);

        let cancel = cancel_msg(2, order_id, "BTCUSDT");
        let responses = route_message(&cancel, &mut session, &exchange, user_id);

        assert!(!responses.is_empty(), "should return a response for cancel");

        let first = &responses[0];
        let bytes = first.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("parseable");
        assert_eq!(parsed.msg_type(), Some("8"), "must be ExecutionReport");
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("4"), "ExecType must be Canceled");
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("4"), "OrdStatus must be Canceled");

        // The mock recorded the cancellation
        assert_eq!(exchange.cancelled.lock().unwrap().len(), 1);
        assert_eq!(exchange.cancelled.lock().unwrap()[0], order_id);
    }

    /// Logon message on a new session produces a Logon response.
    #[test]
    fn test_fix_route_logon_produces_logon_response() {
        let exchange = MockExchange::new();
        let mut session = FixSession::new("EXCHANGE", "CLIENT1", 30);
        let user_id = Uuid::new_v4();

        let msg = logon_msg(1);
        let responses = route_message(&msg, &mut session, &exchange, user_id);

        assert_eq!(responses.len(), 1);
        let parsed = FixMessage::parse(&responses[0].to_bytes()).expect("parseable");
        assert_eq!(parsed.msg_type(), Some(crate::parser::msg_type::LOGON));
        assert!(session.is_logged_on);
    }

    /// Heartbeat message produces a Heartbeat response.
    #[test]
    fn test_fix_route_heartbeat_echoed() {
        let exchange = MockExchange::new();
        let mut session = logged_on_session();
        let user_id = Uuid::new_v4();

        let msg = FixMessageBuilder::new(crate::parser::msg_type::HEARTBEAT, "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, 1u64)
            .build();
        let parsed = FixMessage::parse(&msg.to_bytes()).expect("valid heartbeat");

        let responses = route_message(&parsed, &mut session, &exchange, user_id);

        assert_eq!(responses.len(), 1);
        let r = FixMessage::parse(&responses[0].to_bytes()).expect("parseable");
        assert_eq!(r.msg_type(), Some(crate::parser::msg_type::HEARTBEAT));
    }

    /// Exchange rejection on place_order produces an ExecutionReport(Rejected).
    #[test]
    fn test_fix_route_new_order_exchange_rejection() {
        let exchange = MockExchange::with_place_error("Insufficient balance");
        let mut session = logged_on_session();
        let user_id = Uuid::new_v4();

        let msg = new_order_msg(1, "BTCUSDT", "1");
        let responses = route_message(&msg, &mut session, &exchange, user_id);

        assert!(!responses.is_empty());
        let parsed = FixMessage::parse(&responses[0].to_bytes()).expect("parseable");
        assert_eq!(parsed.msg_type(), Some("8"));
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("8"), "ExecType must be Rejected");
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("8"), "OrdStatus must be Rejected");
    }

    /// Config parsing rejects an invalid bind address.
    #[test]
    fn test_fix_tcp_server_invalid_addr() {
        let cfg = FixTcpServerConfig {
            bind_addr: "not-a-valid-addr".to_string(),
            heartbeat_interval: 30,
            target_comp_id: "EXCHANGE".to_string(),
        };
        assert!(FixTcpServer::new(cfg).is_err());
    }

    // --- Authentication tests ---

    /// Mock exchange that requires authentication.
    struct AuthMockExchange {
        valid_key: String,
        valid_secret: String,
        user_id: Uuid,
    }

    impl AuthMockExchange {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                valid_key: "test-api-key".to_string(),
                valid_secret: "test-api-secret".to_string(),
                user_id: Uuid::new_v4(),
            })
        }
    }

    impl ExchangeAdapter for AuthMockExchange {
        fn place_order(&self, cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
            Ok(vec![MatchEvent::OrderAccepted {
                order_id: cmd.order_id,
                symbol: cmd.symbol.clone(),
                seq: 0,
            }])
        }

        fn cancel_order(&self, cmd: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
            Ok(vec![MatchEvent::OrderCancelled {
                order_id: cmd.order_id,
                symbol: cmd.symbol.clone(),
                remaining_qty: rust_decimal::Decimal::ZERO,
                seq: 0,
            }])
        }

        fn authenticate_fix(&self, api_key: &str, api_secret: &str) -> anyhow::Result<UserId> {
            if api_key == self.valid_key && api_secret == self.valid_secret {
                Ok(self.user_id)
            } else {
                Err(anyhow::anyhow!("Invalid API key or secret"))
            }
        }

        fn fix_auth_required(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_fix_auth_required_trait_default() {
        // Default implementation requires auth and rejects.
        struct Bare;
        impl ExchangeAdapter for Bare {
            fn place_order(&self, _: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
                Ok(vec![])
            }
            fn cancel_order(&self, _: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
                Ok(vec![])
            }
        }
        let bare = Bare;
        assert!(bare.fix_auth_required(), "default must require auth");
        assert!(bare.authenticate_fix("any", "any").is_err(), "default must reject");
    }

    #[test]
    fn test_fix_dev_mode_no_auth() {
        // MockExchange has fix_auth_required = false (dev mode).
        let exchange = MockExchange::new();
        assert!(!exchange.fix_auth_required());
    }

    #[test]
    fn test_fix_auth_mock_valid_credentials() {
        let exchange = AuthMockExchange::new();
        let result = exchange.authenticate_fix("test-api-key", "test-api-secret");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), exchange.user_id);
    }

    #[test]
    fn test_fix_auth_mock_invalid_credentials() {
        let exchange = AuthMockExchange::new();
        let result = exchange.authenticate_fix("wrong-key", "wrong-secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_fix_logon_with_credentials_fields() {
        // Verify that Username(553) and Password(554) fields can be included
        // in a Logon message and read back.
        let msg = FixMessageBuilder::new(crate::parser::msg_type::LOGON, "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, 1u64)
            .field(tag::ENCRYPT_METHOD, "0")
            .field(tag::HEARTBEAT_INT, "30")
            .field(tag::USERNAME, "my-api-key")
            .field(tag::PASSWORD, "my-api-secret")
            .build();
        let parsed = FixMessage::parse(&msg.to_bytes()).expect("valid logon with creds");
        assert_eq!(parsed.get_field(tag::USERNAME), Some("my-api-key"));
        assert_eq!(parsed.get_field(tag::PASSWORD), Some("my-api-secret"));
    }

    #[test]
    fn test_fix_logout_message_includes_reason() {
        let logout = crate::builder::build_logout("EXCHANGE", "CLIENT1", "Auth failed");
        let bytes = logout.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("valid logout");
        assert_eq!(parsed.msg_type(), Some(crate::parser::msg_type::LOGOUT));
        assert_eq!(parsed.get_field(tag::TEXT), Some("Auth failed"));
    }

    // ── S22 I.5 — FIX session place/fill/cancel roundtrip ─────────────────

    use exchange_common::{FillEvent, Side as ExchangeSide, Symbol};
    use rust_decimal_macros::dec;

    /// Mock adapter that reports every placed order as a single full fill.
    /// Used to verify the FIX ExecutionReport(Fill) path end-to-end.
    struct FillingMockExchange {
        placed: Mutex<Vec<PlaceOrderCommand>>,
        cancelled: Mutex<Vec<Uuid>>,
    }

    impl FillingMockExchange {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                placed: Mutex::new(vec![]),
                cancelled: Mutex::new(vec![]),
            })
        }
    }

    impl ExchangeAdapter for FillingMockExchange {
        fn place_order(&self, cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
            self.placed.lock().unwrap().push(cmd.clone());
            // Accept then immediately emit a full fill.
            let fill = FillEvent {
                trade_id: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64,
                symbol: cmd.symbol.clone(),
                price: if cmd.price.is_zero() { dec!(30_000) } else { cmd.price },
                qty: cmd.qty,
                maker_order_id: Uuid::new_v4(),
                taker_order_id: cmd.order_id,
                maker_user_id: Uuid::new_v4(),
                taker_user_id: cmd.user_id,
                taker_side: match cmd.side {
                    exchange_common::Side::Buy => ExchangeSide::Buy,
                    exchange_common::Side::Sell => ExchangeSide::Sell,
                },
                timestamp: chrono::Utc::now(),
                seq: 1,
            };
            Ok(vec![
                MatchEvent::OrderAccepted {
                    order_id: cmd.order_id,
                    symbol: cmd.symbol.clone(),
                    seq: 0,
                },
                MatchEvent::Fill(fill),
            ])
        }

        fn cancel_order(&self, cmd: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
            self.cancelled.lock().unwrap().push(cmd.order_id);
            Ok(vec![MatchEvent::OrderCancelled {
                order_id: cmd.order_id,
                symbol: cmd.symbol.clone(),
                remaining_qty: rust_decimal::Decimal::ZERO,
                seq: 0,
            }])
        }

        fn fix_auth_required(&self) -> bool {
            false
        }
    }

    /// Drive a logged-on FIX session through a realistic institutional
    /// workload: 10 NewOrderSingle, 5 OrderCancelRequest, verify every
    /// ExecutionReport arrives in the expected sequence and the adapter
    /// received the matching commands.
    #[test]
    fn test_fix_session_place_fill_cancel_roundtrip() {
        let exchange = FillingMockExchange::new();
        let mut session = logged_on_session();
        let user_id = Uuid::new_v4();

        // Place 10 orders. Each order generates Accept + Trade reports (2 msgs).
        // Sequence numbers must start at 1 (the session's incoming_seq) and
        // advance monotonically so the session layer doesn't emit a
        // ResendRequest for a gap.
        let mut placed_order_ids: Vec<Uuid> = Vec::with_capacity(10);
        let mut next_seq: u64 = 1;
        for _ in 0..10u64 {
            let ord = new_order_msg(next_seq, "BTCUSDT", "1");
            next_seq += 1;
            let reports = route_message(&ord, &mut session, &exchange, user_id);
            assert_eq!(
                reports.len(),
                2,
                "each place_order should emit ExecReport(New) + ExecReport(Fill), got {}: {:?}",
                reports.len(),
                reports.iter().map(|m| m.get_field(tag::EXEC_TYPE)).collect::<Vec<_>>()
            );
            assert_eq!(reports[0].get_field(tag::EXEC_TYPE), Some("0"), "report 0: New ack");
            assert_eq!(reports[1].get_field(tag::EXEC_TYPE), Some("F"), "report 1: Trade");
            assert_eq!(reports[1].get_field(tag::ORD_STATUS), Some("2"), "Filled status");
            assert_eq!(reports[1].get_field(tag::SYMBOL), Some("BTCUSDT"));
            // Track the real UUID the adapter saw so we can cancel by it below.
            let cmd = exchange.placed.lock().unwrap().last().cloned().unwrap();
            placed_order_ids.push(cmd.order_id);
        }
        assert_eq!(exchange.placed.lock().unwrap().len(), 10);

        // Cancel the first 5 of the placed orders.
        for oid in placed_order_ids.iter().take(5) {
            let cancel = cancel_msg(next_seq, *oid, "BTCUSDT");
            next_seq += 1;
            let reports = route_message(&cancel, &mut session, &exchange, user_id);
            assert_eq!(reports.len(), 1, "cancel emits one ExecReport");
            assert_eq!(reports[0].get_field(tag::EXEC_TYPE), Some("4"), "ExecType=Canceled");
            assert_eq!(reports[0].get_field(tag::ORD_STATUS), Some("4"), "OrdStatus=Canceled");
            assert_eq!(reports[0].get_field(tag::ORDER_ID), Some(oid.to_string().as_str()));
        }
        assert_eq!(exchange.cancelled.lock().unwrap().len(), 5);
        // And the first 5 cancelled ids must match the first 5 placed ids.
        for i in 0..5 {
            assert_eq!(exchange.cancelled.lock().unwrap()[i], placed_order_ids[i]);
        }
    }

    /// Cancel of an order the adapter rejects must fall through without
    /// crashing the session. The session layer returns a safe heartbeat.
    #[test]
    fn test_fix_session_cancel_adapter_error_does_not_break_session() {
        let exchange = MockExchange::with_cancel_error("unknown order");
        let mut session = logged_on_session();
        let user_id = Uuid::new_v4();

        let cancel = cancel_msg(300, Uuid::new_v4(), "BTCUSDT");
        let reports = route_message(&cancel, &mut session, &exchange, user_id);
        // The session emits a heartbeat as the safe fallback; the important
        // contract is that the session is not torn down.
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].msg_type(), Some("0"));
    }
}

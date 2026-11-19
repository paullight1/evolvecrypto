//! Background task spawning for the exchange.
//!
//! Each task is a fire-and-forget tokio loop that operates on shared
//! [`Exchange`] state. Tasks run independently and log errors rather than
//! crashing — a failing liquidation tick should not take down the server.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use uuid::Uuid;

use exchange_algo_execution;
use exchange_common::{CancelOrderCommand, Command, OrderType, PlaceOrderCommand, Side, StpMode, TimeInForce};
use exchange_market_data::MarketDataProvider;
use exchange_common::futures::PositionSide;
use exchange_risk_engine::{LiquidationEngine, LiquidationEvent, LIQUIDATION_FEE_RATE};
use exchange_settlement::{FundingRateRecord, FundingSettlement};
use exchange_trading_bots::BotStatus;

use crate::exchange::{Exchange, MarketEvent, PostFillEvent, PrivateEvent};

// ---------------------------------------------------------------------------
// Supervisor helper
// ---------------------------------------------------------------------------

/// Spawn a supervised background task that restarts on panic.
///
/// The factory closure `task_fn` is called on every (re)start and must return
/// a pinned future that runs the task body. When the inner task panics it is
/// restarted after a 1-second back-off. A clean exit also triggers a restart
/// (tasks are expected to run forever). The supervisor stops only when the
/// inner task is cancelled (JoinError::is_cancelled).
///
/// This mirrors the pattern already used by `spawn_liquidation_monitor`.
pub fn supervised_spawn<F>(name: &'static str, task_fn: F) -> JoinHandle<()>
where
    F: Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            let result = tokio::spawn(task_fn()).await;
            match result {
                Ok(()) => {
                    tracing::warn!(task = name, "background task exited cleanly — restarting");
                }
                Err(e) if e.is_panic() => {
                    tracing::error!(
                        task = name,
                        error = %e,
                        "background task PANICKED — restarting in 1s"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    // Task was cancelled (aborted by the caller) — stop the supervisor.
                    tracing::info!(task = name, "background task cancelled — supervisor stopping");
                    break;
                }
            }
        }
    })
}

/// Spawn a supervised background task that handles a graceful-shutdown signal.
///
/// Unlike [`supervised_spawn`], a **clean exit** from the inner task is treated
/// as an intentional shutdown (e.g. the inner task received the shutdown watch
/// channel signal and returned).  Only panics trigger a restart; clean exits
/// and cancellations both stop the supervisor loop.
///
/// Use this for tasks that own a `watch::Receiver<bool>` shutdown signal and
/// call `return` when the signal fires.
pub fn supervised_spawn_with_shutdown<F>(name: &'static str, task_fn: F) -> JoinHandle<()>
where
    F: Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            let result = tokio::spawn(task_fn()).await;
            match result {
                Ok(()) => {
                    // Clean exit = shutdown signal was handled — stop supervisor.
                    tracing::info!(task = name, "background task exited cleanly — supervisor stopping");
                    break;
                }
                Err(e) if e.is_panic() => {
                    tracing::error!(
                        task = name,
                        error = %e,
                        "background task PANICKED — restarting in 1s"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    // Task was cancelled (aborted) — stop the supervisor.
                    tracing::info!(task = name, "background task cancelled — supervisor stopping");
                    break;
                }
            }
        }
    })
}

/// Check all positions for liquidation every `interval_ms` milliseconds.
///
/// When a position is liquidatable, logs a warning and broadcasts a
/// [`MarketEvent::Liquidation`] on the public feed.
///
/// Wrapped in a supervisor loop: if the inner task panics it is restarted
/// after a 1-second back-off. A clean exit also triggers a restart (tasks
/// are expected to run forever). The supervisor only stops when the inner
/// task is cancelled (aborted by the caller).
pub fn spawn_liquidation_monitor(
    exchange: Arc<Exchange>,
    interval_ms: u64,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let ex = Arc::clone(&exchange);
            let mut rx = shutdown_rx.clone();
            let result = tokio::spawn(async move {
                let mut ticker = interval(Duration::from_millis(interval_ms));
                let engine = LiquidationEngine::new(50);
                run_liquidation_monitor_inner(ex, &mut ticker, &engine, &mut rx).await;
            })
            .await;

            match result {
                Ok(()) => {
                    // Clean exit means shutdown was signalled — stop the supervisor.
                    tracing::info!("liquidation_monitor exited cleanly — supervisor stopping");
                    break;
                }
                Err(e) if e.is_panic() => {
                    tracing::error!(
                        panic = %e,
                        "liquidation_monitor PANICKED — restarting in 1s"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    // Task was cancelled (aborted) — stop the supervisor.
                    tracing::info!("liquidation_monitor cancelled — supervisor stopping");
                    break;
                }
            }
        }
    })
}

/// Inner loop body for the liquidation monitor, extracted so the supervisor
/// can spawn it in a separate task and catch panics via [`JoinHandle::await`].
async fn run_liquidation_monitor_inner(
    exchange: Arc<Exchange>,
    ticker: &mut tokio::time::Interval,
    engine: &LiquidationEngine,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("liquidation_monitor: shutdown signal received — exiting");
                    return;
                }
            }
        }

        let mut mark_prices_full = exchange.get_mark_prices();

        if mark_prices_full.is_empty() {
            tracing::error!(
                "LIQUIDATION MONITOR: no mark prices available — ALL positions are unmonitored! \
                 Falling back to last traded price for each known symbol."
            );

            // Fallback: use the last traded price from market data for each
            // registered symbol so the monitor is never completely blind.
            let now_fallback = chrono::Utc::now();
            for ticker in exchange.market_data.all_tickers() {
                let sym = ticker.symbol.clone();
                let fallback_price = ticker.last_price;
                if fallback_price > Decimal::ZERO {
                    mark_prices_full.insert(sym.clone(), exchange_common::futures::MarkPrice {
                        symbol: sym,
                        price: fallback_price,
                        index_price: fallback_price,
                        last_price: fallback_price,
                        timestamp: now_fallback,
                    });
                }
            }

            // If we still have nothing after the fallback, skip this tick.
            if mark_prices_full.is_empty() {
                continue;
            }
        }

        // LiquidationEngine expects Price (Decimal), not MarkPrice
        let mark_prices: std::collections::HashMap<_, _> = mark_prices_full
            .iter()
            .map(|(sym, mp)| (sym.clone(), mp.price))
            .collect();

            let positions: Vec<_> = {
                let pm = exchange.positions.lock();
                pm.all_positions().into_iter().cloned().collect()
            };

            if positions.is_empty() {
                continue;
            }

            // S17: load_full() returns Arc<HashMap> — no mutex, no clone.
            let perp_configs_snapshot = exchange.perp_configs.load_full();

            // Auto-add margin: top up isolated positions that are within 20% of
            // liquidation before running the liquidation check proper.
            for pos in &positions {
                if !pos.auto_add_margin {
                    continue;
                }
                if pos.margin_mode != exchange_common::futures::MarginMode::Isolated {
                    continue;
                }
                let mark = match mark_prices.get(&pos.symbol) {
                    Some(&p) => p,
                    None => continue,
                };
                let config = match perp_configs_snapshot.get(&pos.symbol) {
                    Some(c) => c,
                    None => continue,
                };
                let mmr = config.effective_mmr(pos.notional(mark));
                let maintenance_margin = pos.notional(mark) * mmr;
                let effective = pos.effective_margin(mark);

                // Top up when within 20% of the maintenance margin threshold.
                if effective < maintenance_margin * rust_decimal::Decimal::from(12) / rust_decimal::Decimal::from(10) {
                    // Target: restore to 2x the maintenance margin.
                    let needed = maintenance_margin * rust_decimal::Decimal::TWO - effective;
                    if needed <= rust_decimal::Decimal::ZERO {
                        continue;
                    }
                    let available = exchange
                        .accounts
                        .get_balance(pos.user_id, "USDT")
                        .map(|b| b.available())
                        .unwrap_or(rust_decimal::Decimal::ZERO);
                    let add_amount = needed.min(available);
                    if add_amount <= rust_decimal::Decimal::ZERO {
                        continue;
                    }
                    // Deduct from trading balance.
                    let withdraw_ok = exchange
                        .accounts
                        .withdraw(pos.user_id, "USDT", add_amount)
                        .is_ok();
                    if !withdraw_ok {
                        continue;
                    }
                    // Credit to position margin.
                    let _ = exchange
                        .positions
                        .lock()
                        .add_margin_to_position(pos.user_id, &pos.symbol, pos.position_idx, add_amount);
                    tracing::info!(
                        user_id = %pos.user_id,
                        symbol = %pos.symbol,
                        amount = %add_amount,
                        "auto-added margin to position",
                    );
                }
            }

            let batch = engine.check_all_positions(
                positions.iter(),
                &mark_prices,
                &perp_configs_snapshot,
            );

            for order in batch.orders {
                // Re-read the live position before acting — it may have been closed
                // or topped up by a concurrent liquidation earlier in this same batch.
                {
                    let pm = exchange.positions.lock();
                    let live = pm.get_position(order.user_id, &order.symbol, order.position_idx);
                    match live {
                        None => {
                            tracing::info!(
                                user_id = %order.user_id,
                                symbol = %order.symbol,
                                "liquidation skipped — position no longer exists (already closed)"
                            );
                            continue;
                        }
                        Some(live_pos) => {
                            let mark = match mark_prices.get(&order.symbol) {
                                Some(&p) => p,
                                None => continue,
                            };
                            let config = match perp_configs_snapshot.get(&order.symbol) {
                                Some(c) => c,
                                None => continue,
                            };
                            let mmr = config.effective_mmr(live_pos.notional(mark));
                            let maintenance_margin = live_pos.notional(mark) * mmr;
                            if live_pos.effective_margin(mark) >= maintenance_margin {
                                tracing::info!(
                                    user_id = %order.user_id,
                                    symbol = %order.symbol,
                                    "liquidation skipped — position margin ratio is now healthy"
                                );
                                continue;
                            }
                        }
                    }
                }

                tracing::warn!(
                    user_id = %order.user_id,
                    symbol = %order.symbol,
                    side = ?order.side,
                    size = %order.size,
                    mark_price = %order.mark_price,
                    bankruptcy_price = %order.bankruptcy_price,
                    "liquidating position — placing market close order"
                );

                // The closing side is opposite to the position side:
                // Long position → Sell to close; Short position → Buy to close.
                let close_side = match order.side {
                    PositionSide::Long => Side::Sell,
                    PositionSide::Short => Side::Buy,
                };

                let cmd = PlaceOrderCommand {
                    order_id: Uuid::new_v4(),
                    user_id: order.user_id,
                    symbol: order.symbol.clone(),
                    side: close_side,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Ioc,
                    price: Decimal::ZERO,
                    qty: order.size,
                    reduce_only: true,
                    expire_at: None,
                    timestamp: Utc::now(),
                    position_idx: order.position_idx,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: StpMode::default(),
                    request_expire_time: None,
                };

                match exchange.place_order(cmd) {
                    Ok(events) => {
                        // Determine the fill price and actual filled qty from matching engine events.
                        // For a market IOC order the fill may be partial if the book is thin.
                        let (fill_price, total_filled_qty) = events.iter()
                            .filter_map(|e| {
                                if let exchange_common::MatchEvent::Fill(f) = e {
                                    Some((f.price, f.qty))
                                } else {
                                    None
                                }
                            })
                            .fold((order.mark_price, Decimal::ZERO), |(_, acc_qty), (price, qty)| {
                                (price, acc_qty + qty)
                            });

                        // Process insurance fund impact at the achieved fill price and actual qty.
                        let current_fund_balance = exchange
                            .insurance_funds
                            .lock().unwrap_or_else(|e| e.into_inner())
                            .get(&order.symbol)
                            .map(|f| f.balance())
                            .unwrap_or(Decimal::ZERO);

                        // Use the actual filled qty rather than the full position size so that
                        // a partial IOC fill doesn't over-count surplus/deficit.
                        let mut liq_order_for_fund = order.clone();
                        liq_order_for_fund.size = total_filled_qty;
                        let fund_update = engine.process_insurance_fund(
                            &liq_order_for_fund,
                            fill_price,
                            current_fund_balance,
                        );

                        // Apply insurance fund delta and record history.
                        {
                            let mut funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(fund) = funds.get_mut(&order.symbol) {
                                let (reason, applied_delta) = if fund_update.delta >= Decimal::ZERO {
                                    let _ = fund.add_surplus(fund_update.delta);
                                    ("liquidation_surplus", fund_update.delta)
                                } else {
                                    // cover_deficit will log a warning if exhausted.
                                    let covered = fund_update.delta.abs().min(fund.balance());
                                    let _ = fund.cover_deficit(covered);
                                    ("liquidation_deficit", -covered)
                                };
                                let balance_after = fund.balance();
                                drop(funds);
                                exchange.record_insurance_fund_event(
                                    &order.symbol,
                                    applied_delta,
                                    balance_after,
                                    reason,
                                );
                            }
                        }

                        if fund_update.socialised_loss > Decimal::ZERO {
                            tracing::warn!(
                                symbol = %order.symbol,
                                socialised_loss = %fund_update.socialised_loss,
                                "insurance fund exhausted — triggering ADL"
                            );

                            // Record socialized loss event in insurance fund history.
                            let fund_balance_now = exchange
                                .insurance_funds
                                .lock().unwrap_or_else(|e| e.into_inner())
                                .get(&order.symbol)
                                .map(|f| f.balance())
                                .unwrap_or(Decimal::ZERO);
                            exchange.record_insurance_fund_event(
                                &order.symbol,
                                -fund_update.socialised_loss,
                                fund_balance_now,
                                "socialized_loss",
                            );

                            // Re-read live positions for ADL ranking — the stale snapshot
                            // from the top of this tick may contain already-liquidated
                            // positions and would produce a wrong ranking.
                            let live_positions_for_adl: Vec<_> = {
                                let pm = exchange.positions.lock();
                                pm.all_positions().into_iter().cloned().collect()
                            };
                            let ranking = exchange_risk_engine::AdlRanking;
                            let candidates = ranking.rank_positions(
                                &live_positions_for_adl,
                                &mark_prices,
                                order.side,
                                &order.symbol,
                            );

                            if !candidates.is_empty() {
                                let adl_orders = exchange_risk_engine::calculate_adl(
                                    fund_update.socialised_loss,
                                    &candidates,
                                    order.bankruptcy_price,
                                );

                                for adl in &adl_orders {
                                    // Guard: verify the ADL target position still exists and
                                    // is still profitable (in-the-money) before reducing it.
                                    // A previous ADL iteration in this loop may have already
                                    // fully closed the position, which would constitute double-ADL.
                                    {
                                        let pm = exchange.positions.lock();
                                        match pm.get_position(adl.user_id, &adl.symbol, 0) {
                                            None => {
                                                tracing::info!(
                                                    user_id = %adl.user_id,
                                                    symbol = %adl.symbol,
                                                    "ADL skipped — position no longer exists"
                                                );
                                                continue;
                                            }
                                            Some(live_pos) => {
                                                let mark = match mark_prices.get(&adl.symbol) {
                                                    Some(&p) => p,
                                                    None => continue,
                                                };
                                                // ADL targets must be profitable — unrealized PnL > 0.
                                                if live_pos.unrealized_pnl(mark) <= Decimal::ZERO {
                                                    tracing::info!(
                                                        user_id = %adl.user_id,
                                                        symbol = %adl.symbol,
                                                        "ADL skipped — position is no longer profitable"
                                                    );
                                                    continue;
                                                }
                                            }
                                        }
                                    }

                                    let result = {
                                        let mut pm = exchange.positions.lock();
                                        pm.reduce(
                                            adl.user_id,
                                            &adl.symbol,
                                            adl.close_qty,
                                            adl.close_price,
                                            0,
                                        )
                                    };

                                    match result {
                                        Ok(close_result) => {
                                            if close_result.released_margin > Decimal::ZERO {
                                                let _ = exchange.accounts
                                                    .deposit(adl.user_id, "USDT", close_result.released_margin + close_result.realized_pnl);
                                            }

                                            tracing::warn!(
                                                user_id = %adl.user_id,
                                                symbol = %adl.symbol,
                                                qty = %adl.close_qty,
                                                price = %adl.close_price,
                                                realized_pnl = %close_result.realized_pnl,
                                                "ADL executed — position reduced"
                                            );

                                            let _ = exchange.private_tx.send(PrivateEvent::AdlClose {
                                                user_id: adl.user_id.to_string(),
                                                symbol: adl.symbol.to_string(),
                                                qty: adl.close_qty,
                                                price: adl.close_price,
                                                realized_pnl: close_result.realized_pnl,
                                            });
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                user_id = %adl.user_id,
                                                symbol = %adl.symbol,
                                                error = %e,
                                                "ADL reduce failed"
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        tracing::warn!(
                            user_id = %order.user_id,
                            symbol = %order.symbol,
                            fill_price = %fill_price,
                            insurance_delta = %fund_update.delta,
                            "liquidation executed"
                        );
                    }
                    Err(e) => {
                        // No liquidity or order rejected — will retry on next tick.
                        tracing::error!(
                            user_id = %order.user_id,
                            symbol = %order.symbol,
                            error = %e,
                            "liquidation order failed — will retry next tick"
                        );
                        continue;
                    }
                }

                // Record liquidation event (partial or full).
                let original_pos_size = positions
                    .iter()
                    .find(|p| p.user_id == order.user_id && p.symbol == order.symbol && p.position_idx == order.position_idx)
                    .map(|p| p.size)
                    .unwrap_or(order.size);
                let is_partial = order.size < original_pos_size;
                let liq_fee_rate: Decimal = LIQUIDATION_FEE_RATE.parse().unwrap_or(Decimal::ZERO);
                let liq_fee = order.size * order.mark_price * liq_fee_rate;

                let event = LiquidationEvent {
                    id: Uuid::new_v4(),
                    user_id: order.user_id,
                    symbol: order.symbol.clone(),
                    side: order.side,
                    qty_liquidated: order.size,
                    price: order.mark_price,
                    is_partial,
                    fee: liq_fee,
                    timestamp: Utc::now(),
                };
                {
                    let mut events = exchange.liquidation_events.lock().unwrap_or_else(|e| e.into_inner());
                    if events.len() >= 50_000 {
                        events.drain(..1_000);
                    }
                    events.push(event);
                }

                // Charge liquidation fee: debit user, credit insurance fund.
                if liq_fee > Decimal::ZERO {
                    // Debit user (best effort — user may have insufficient balance).
                    let _ = exchange.accounts.withdraw(order.user_id, "USDT", liq_fee);
                    // Credit insurance fund for this symbol.
                    let mut ins_funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(fund) = ins_funds.get_mut(&order.symbol) {
                        let _ = fund.add_surplus(liq_fee);
                        tracing::info!(
                            symbol = %order.symbol,
                            user_id = %order.user_id,
                            fee = %liq_fee,
                            "liquidation fee charged → insurance fund"
                        );
                    }
                }

                // Broadcast public liquidation event regardless of fill completeness.
                let _ = exchange.market_tx.send(MarketEvent::Liquidation {
                    symbol: order.symbol.to_string(),
                    side: format!("{:?}", order.side),
                    size: order.size,
                    price: order.mark_price,
                    timestamp: Utc::now().timestamp_millis(),
                });

                // NOTIF-5: Push notification for liquidation.
                exchange.send_push_notification(
                    order.user_id,
                    "Position Liquidated",
                    &format!(
                        "Your {:?} {} position ({} contracts) was liquidated at {}",
                        order.side, order.symbol, order.size, order.mark_price
                    ),
                    exchange_admin::NotificationType::PositionLiquidated,
                    serde_json::json!({
                        "symbol": order.symbol.as_str(),
                        "side": format!("{:?}", order.side),
                        "size": order.size.to_string(),
                        "mark_price": order.mark_price.to_string(),
                        "is_partial": is_partial,
                    }),
                );
            }

            // Cross-margin liquidation: group users in cross mode, compute their
            // total equity, and liquidate the most-losing position first if the account
            // equity has fallen below the sum of maintenance margins.
            {
                use exchange_common::futures::MarginMode;
                use std::collections::HashSet;

                // Collect unique cross-margin users from open positions.
                let cross_users: HashSet<uuid::Uuid> = positions
                    .iter()
                    .filter(|p| p.margin_mode == MarginMode::Cross)
                    .map(|p| p.user_id)
                    .collect();

                for user_id in cross_users {
                    let user_positions: Vec<&exchange_common::futures::Position> = positions
                        .iter()
                        .filter(|p| p.user_id == user_id && p.margin_mode == MarginMode::Cross)
                        .collect();

                    let total_equity = exchange.cross_margin_balance(user_id);

                    let cross_orders = engine.check_cross_margin_liquidation(
                        &user_positions,
                        &mark_prices,
                        &perp_configs_snapshot,
                        total_equity,
                    );

                    if cross_orders.is_empty() {
                        continue;
                    }

                    // In cross mode: liquidate the most-losing position first.
                    // Sort by unrealized PnL ascending (most negative first).
                    let mut sorted_orders = cross_orders;
                    sorted_orders.sort_by(|a, b| {
                        let pnl_a = user_positions
                            .iter()
                            .find(|p| p.symbol == a.symbol)
                            .map(|p| p.unrealized_pnl(a.mark_price))
                            .unwrap_or(Decimal::ZERO);
                        let pnl_b = user_positions
                            .iter()
                            .find(|p| p.symbol == b.symbol)
                            .map(|p| p.unrealized_pnl(b.mark_price))
                            .unwrap_or(Decimal::ZERO);
                        pnl_a.cmp(&pnl_b)
                    });

                    // Only liquidate the first (most underwater) position; re-check next tick.
                    if let Some(order) = sorted_orders.first() {
                        tracing::warn!(
                            user_id = %order.user_id,
                            symbol = %order.symbol,
                            total_equity = %total_equity,
                            "cross-margin liquidation triggered"
                        );

                        let close_side = match order.side {
                            PositionSide::Long => Side::Sell,
                            PositionSide::Short => Side::Buy,
                        };

                        let cmd = PlaceOrderCommand {
                            order_id: Uuid::new_v4(),
                            user_id: order.user_id,
                            symbol: order.symbol.clone(),
                            side: close_side,
                            order_type: OrderType::Market,
                            time_in_force: TimeInForce::Ioc,
                            price: Decimal::ZERO,
                            qty: order.size,
                            reduce_only: true,
                            expire_at: None,
                            timestamp: Utc::now(),
                            position_idx: order.position_idx,
                            iceberg_qty: None,
                            order_link_id: None,
                            slippage_tolerance: None,
                            close_position: false,
                            stp_mode: StpMode::default(),
                            request_expire_time: None,
                        };

                        if let Err(e) = exchange.place_order(cmd) {
                            tracing::error!(
                                user_id = %order.user_id,
                                symbol = %order.symbol,
                                error = %e,
                                "cross-margin liquidation order failed"
                            );
                        } else {
                            // Record cross-margin liquidation event (always full close for cross).
                            let liq_fee_rate: Decimal = LIQUIDATION_FEE_RATE.parse().unwrap_or(Decimal::ZERO);
                            let liq_fee = order.size * order.mark_price * liq_fee_rate;
                            let cross_event = LiquidationEvent {
                                id: Uuid::new_v4(),
                                user_id: order.user_id,
                                symbol: order.symbol.clone(),
                                side: order.side,
                                qty_liquidated: order.size,
                                price: order.mark_price,
                                is_partial: false,
                                fee: liq_fee,
                                timestamp: Utc::now(),
                            };
                            {
                                let mut events = exchange.liquidation_events.lock().unwrap_or_else(|e| e.into_inner());
                                if events.len() >= 50_000 {
                                    events.drain(..1_000);
                                }
                                events.push(cross_event);
                            }

                            let _ = exchange.market_tx.send(MarketEvent::Liquidation {
                                symbol: order.symbol.to_string(),
                                side: format!("{:?}", order.side),
                                size: order.size,
                                price: order.mark_price,
                                timestamp: Utc::now().timestamp_millis(),
                            });
                        }
                    }
                }
            }

            // Check all positions for liquidation-warning notifications (>70% margin usage).
            for pos in &positions {
                let mark = match mark_prices.get(&pos.symbol) {
                    Some(&p) => p,
                    None => continue,
                };
                let liq_price = match pos.liquidation_price() {
                    Some(p) => p,
                    None => continue,
                };
                // Margin usage = how close we are to liquidation.
                // For a long: usage = (entry - mark) / (entry - liq), capped to [0,1].
                // For a short: usage = (mark - entry) / (liq - entry), capped to [0,1].
                // We use a simpler proxy: distance-to-liq as a fraction of the liq band.
                let band = (pos.entry_price - liq_price).abs();
                if band.is_zero() {
                    continue;
                }
                let distance = (mark - liq_price).abs();
                let margin_usage_pct = {
                    let raw = rust_decimal::Decimal::ONE - (distance / band);
                    let clamped = raw.max(rust_decimal::Decimal::ZERO)
                        .min(rust_decimal::Decimal::ONE);
                    // Convert to f64 percentage
                    clamped.try_into().unwrap_or(0.0_f64) * 100.0
                };

                if margin_usage_pct >= 70.0 {
                    exchange.notifications.lock().unwrap_or_else(|e| e.into_inner()).liquidation_warning(
                        pos.user_id,
                        &pos.symbol,
                        margin_usage_pct,
                    );
                }
            }
        }
}

/// Settle funding payments every time a funding interval is due.
///
/// Checks every 60 seconds whether a settlement slot has been reached.
/// Sample premium index every 60 seconds for TWAP funding rate calculation.
///
/// For each perpetual symbol, reads the current mid-price (perp proxy) and
/// index price, computes `(perp − index) / index`, and stores the sample in
/// the symbol's [`PremiumBuffer`]. At settlement time, the funding settler
/// reads the 8h TWAP instead of using an instantaneous snapshot.
///
/// Reference: Bybit perpetual swap documentation, "Funding Rate" section.
pub fn spawn_premium_sampler(
    exchange: Arc<Exchange>,
) -> JoinHandle<()> {
    supervised_spawn("premium_sampler", move || {
        let ex = Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;

                let perp_symbols: Vec<exchange_common::Symbol> = ex
                    .perp_configs
                    .load()
                    .keys()
                    .cloned()
                    .collect();

                for symbol in &perp_symbols {
                    // Get perp mid-price from order book.
                    let perp_mid = {
                        let pool = ex.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                        match pool.snapshot(symbol, 1) {
                            Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                                (Some((bid, _)), Some((ask, _))) => (*bid + *ask) / Decimal::TWO,
                                _ => continue, // no two-sided book → skip
                            },
                            Err(_) => continue,
                        }
                    };

                    // Get index price.
                    let index_price = {
                        let agg = ex.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                        match agg.calculate_index(symbol) {
                            Ok(ip) => ip.price,
                            Err(_) => continue, // no index → skip
                        }
                    };

                    // Record premium sample.
                    let mut buffers = ex.premium_buffers.lock().unwrap_or_else(|e| e.into_inner());
                    let buf = buffers
                        .entry(symbol.clone())
                        .or_default();
                    if let Err(e) = buf.record_sample(perp_mid, index_price) {
                        tracing::warn!(
                            symbol = %symbol,
                            error = %e,
                            "premium sampler: failed to record sample"
                        );
                    }
                }
            }
        })
    })
}

/// When due: calculates the rate, settles payments for all open positions,
/// broadcasts [`MarketEvent::FundingUpdate`] and [`PrivateEvent::FundingPayment`]
/// for each affected position.
pub fn spawn_funding_settler(
    exchange: Arc<Exchange>,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    supervised_spawn_with_shutdown("funding_settler", move || {
        let exchange = Arc::clone(&exchange);
        let mut shutdown_rx = shutdown_rx.clone();
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));

        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("funding_settler: shutdown signal received — exiting");
                        return;
                    }
                }
            }

            let now = chrono::Utc::now();

            let is_due = {
                let scheduler = exchange.funding_scheduler.lock().unwrap_or_else(|e| e.into_inner());
                match scheduler.is_settlement_due(now, 30) {
                    Ok(due) => due,
                    Err(e) => {
                        tracing::error!(error = %e, "funding scheduler error");
                        continue;
                    }
                }
            };

            if !is_due {
                continue;
            }

            tracing::info!("funding settlement due — processing all symbols");

            // S17: load_full() returns Arc<HashMap> — no mutex, no clone.
            let perp_configs_funding = exchange.perp_configs.load_full();
            for (symbol, config) in perp_configs_funding.iter() {
                // Get mark price; skip symbol if unavailable
                let mark_price = match exchange.get_mark_price(symbol) {
                    Some(mp) => mp,
                    None => {
                        tracing::warn!(symbol = %symbol, "no mark price for funding settlement — skipping");
                        continue;
                    }
                };

                let index_price = {
                    let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                    match agg.calculate_index(symbol) {
                        Ok(ip) => ip.price,
                        Err(e) => {
                            tracing::warn!(symbol = %symbol, error = %e, "no index price — skipping funding");
                            continue;
                        }
                    }
                };

                // Use TWAP premium if available (sampled by premium_sampler task),
                // otherwise fall back to instantaneous premium.
                let twap = {
                    let buffers = exchange.premium_buffers.lock().unwrap_or_else(|e| e.into_inner());
                    buffers.get(symbol).and_then(|b| b.twap_premium())
                };

                let funding_rate = match exchange_settlement::calculate_funding_rate_twap(
                    mark_price.price,
                    index_price,
                    config.max_funding_rate,
                    twap,
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(symbol = %symbol, error = %e, "funding rate calculation failed");
                        continue;
                    }
                };

                // Collect positions for this symbol
                let positions: Vec<_> = {
                    let pm = exchange.positions.lock();
                    pm.all_positions()
                        .into_iter()
                        .filter(|p| &p.symbol == symbol)
                        .cloned()
                        .collect()
                };

                let payments = FundingSettlement::settle_with_config(
                    &positions,
                    funding_rate,
                    mark_price.price,
                    now,
                    config.is_inverse(),
                    config.contract_size,
                    &config.settle_coin,
                );

                // Record rate + payments in history
                {
                    let mut history_map = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());
                    let history = history_map.entry(symbol.clone()).or_default();
                    history.record_rate(FundingRateRecord {
                        symbol: symbol.clone(),
                        rate: funding_rate,
                        mark_price: mark_price.price,
                        index_price,
                        calculated_at: now,
                    });
                    history.record_payments(payments.clone());
                }

                // DB write-behind: persist funding rate
                {
                    let maybe_pool = exchange.db_pool.get().cloned();
                    if let Some(pool) = maybe_pool {
                        let sym_str = symbol.to_string();
                        let rate_val = funding_rate;
                        let mp = mark_price.price;
                        let ip = index_price;
                        let settled = now;
                        tokio::spawn(async move {
                            if let Err(e) = exchange_db::funding_repo::FundingRepo::insert_rate(
                                &pool, &sym_str, rate_val, mp, ip, settled,
                            ).await {
                                tracing::error!(symbol = %sym_str, error = %e, "failed to persist funding rate");
                            }
                        });
                    }
                }

                // Broadcast public funding update
                let next_time = {
                    let scheduler = exchange.funding_scheduler.lock().unwrap_or_else(|e| e.into_inner());
                    scheduler
                        .next_funding_time(now)
                        .map(|t| t.timestamp())
                        .unwrap_or(0)
                };

                let _ = exchange.market_tx.send(MarketEvent::FundingUpdate {
                    symbol: symbol.to_string(),
                    rate: funding_rate,
                    mark_price: mark_price.price,
                    next_time,
                });

                // Also broadcast on the `funding.{symbol}` WS channel as a
                // FundingRateUpdate so clients subscribed to that topic receive
                // the current rate immediately after each settlement.
                let _ = exchange.market_tx.send(MarketEvent::FundingRateUpdate {
                    symbol: symbol.to_string(),
                    rate: funding_rate,
                    mark_price: mark_price.price,
                    next_funding_time: next_time * 1_000, // convert to ms
                    timestamp: now.timestamp_millis(),
                });

                // Credit / debit user balances and broadcast private payment events.
                for payment in &payments {
                    if payment.amount.is_zero() {
                        // No cash movement needed, but still broadcast the event.
                    } else if payment.amount > rust_decimal::Decimal::ZERO {
                        // User receives funding: credit their balance in the settlement asset.
                        let settle_asset = &payment.settle_asset;
                        let result = exchange
                            .accounts
                            .deposit(payment.user_id, settle_asset, payment.amount);
                        match result {
                            Ok(()) => {
                                tracing::info!(
                                    user_id = %payment.user_id,
                                    symbol = %payment.symbol,
                                    asset = %settle_asset,
                                    amount = %payment.amount,
                                    "funding payment credited"
                                );
                                exchange.append_to_wal(&exchange_common::Command::FundingPayment {
                                    user_id: payment.user_id,
                                    asset: settle_asset.clone(),
                                    amount: payment.amount,
                                    rate: funding_rate,
                                    symbol: payment.symbol.to_string(),
                                });
                            }
                            Err(e) => tracing::warn!(
                                user_id = %payment.user_id,
                                symbol = %payment.symbol,
                                amount = %payment.amount,
                                error = %e,
                                "failed to credit funding payment — skipping"
                            ),
                        }
                    } else {
                        // User pays funding: debit their balance in the settlement asset.
                        let settle_asset = &payment.settle_asset;
                        let debit = payment.amount.abs();
                        let result = exchange
                            .accounts
                            .withdraw(payment.user_id, settle_asset, debit);
                        match result {
                            Ok(()) => {
                                tracing::info!(
                                    user_id = %payment.user_id,
                                    symbol = %payment.symbol,
                                    amount = %payment.amount,
                                    "funding payment debited"
                                );
                                exchange.append_to_wal(&exchange_common::Command::FundingPayment {
                                    user_id: payment.user_id,
                                    asset: settle_asset.clone(),
                                    amount: payment.amount, // negative = paid
                                    rate: funding_rate,
                                    symbol: payment.symbol.to_string(),
                                });
                            }
                            Err(e) => {
                                // Balance insufficient — track as unpaid debt on the position.
                                // The debt reduces effective margin so the liquidation engine
                                // will process this position sooner: correct behaviour for an
                                // under-margined account that cannot meet its funding obligation.
                                let debt = payment.amount.abs();
                                {
                                    let mut pm = exchange.positions.lock();
                                    if let Some(pos) = pm.get_position_mut(payment.user_id, &payment.symbol, 0) {
                                        pos.unpaid_funding_debt += debt;
                                        tracing::warn!(
                                            user_id = %payment.user_id,
                                            symbol = %payment.symbol,
                                            payment = %debt,
                                            total_debt = %pos.unpaid_funding_debt,
                                            error = %e,
                                            "funding payment failed — debt tracked on position"
                                        );
                                    } else {
                                        tracing::warn!(
                                            user_id = %payment.user_id,
                                            symbol = %payment.symbol,
                                            payment = %debt,
                                            error = %e,
                                            "funding payment failed — no position found to track debt"
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Write-behind ledger: funding payment (user ↔ insurance fund as intermediary).
                    // Funding is zero-sum between longs and shorts; the insurance fund balance
                    // nets to zero across the full settlement cycle.
                    if !payment.amount.is_zero() {
                        let maybe_pool = exchange.db_pool.get().cloned();
                        if let Some(pool) = maybe_pool {
                            let uid = payment.user_id;
                            let amount = payment.amount;
                            let funding_id = format!("funding-{}-{}", payment.symbol, now.timestamp());
                            tokio::spawn(async move {
                                // Insurance fund (0x02) acts as the counterparty per payment
                                // so each entry is a balanced 2-entry transaction. The sum of
                                // all funding payments across a settlement cycle is zero, so
                                // the insurance fund balance nets back to unchanged.
                                let insurance = Uuid::from_u128(0x02);
                                let abs_amount = amount.abs();
                                let (user_amount, insurance_amount) = if amount > rust_decimal::Decimal::ZERO {
                                    // User receives: credit user, debit insurance
                                    (abs_amount, -abs_amount)
                                } else {
                                    // User pays: debit user, credit insurance
                                    (-abs_amount, abs_amount)
                                };
                                if let Err(e) = exchange_db::ledger::LedgerService::post(&pool, vec![
                                    exchange_db::ledger::LedgerEntry {
                                        account_id: uid,
                                        account_type: "user_derivatives".into(),
                                        asset: "USDT".into(),
                                        amount: user_amount,
                                        entry_type: exchange_db::ledger::EntryType::FundingPayment,
                                        reference_id: Some(funding_id.clone()),
                                        reference_type: Some("funding".into()),
                                        description: None,
                                    },
                                    exchange_db::ledger::LedgerEntry {
                                        account_id: insurance,
                                        account_type: "system".into(),
                                        asset: "USDT".into(),
                                        amount: insurance_amount,
                                        entry_type: exchange_db::ledger::EntryType::FundingPayment,
                                        reference_id: Some(funding_id),
                                        reference_type: Some("funding".into()),
                                        description: None,
                                    },
                                ]).await {
                                    tracing::error!(
                                        user_id = %uid,
                                        error = %e,
                                        "ledger write failed for funding payment"
                                    );
                                }
                            });
                        }
                    }

                    // Update position funding_accumulated (in-memory)
                    {
                        let mut pm = exchange.positions.lock();
                        pm.add_funding(payment.user_id, &payment.symbol, payment.amount, 0);
                    }

                    // DB write-behind: persist funding payment + update position
                    if !payment.amount.is_zero() {
                        let pos_db_id = exchange.position_db_ids
                            .lock().unwrap_or_else(|e| e.into_inner())
                            .get(&(payment.user_id, payment.symbol.clone()))
                            .copied();
                        if let Some(db_id) = pos_db_id {
                            let maybe_pool = exchange.db_pool.get().cloned();
                            if let Some(pool) = maybe_pool {
                                let uid = payment.user_id;
                                let sym = payment.symbol.to_string();
                                let pay_rate = payment.rate;
                                let pay_amount = payment.amount;
                                let pay_mp = payment.mark_price;
                                let pay_ts = payment.settled_at;
                                tokio::spawn(async move {
                                    if let Err(e) = exchange_db::funding_repo::FundingRepo::insert_payment(
                                        &pool, uid, &sym, db_id, pay_rate, pay_amount, pay_mp, None, pay_ts,
                                    ).await {
                                        tracing::error!(user_id = %uid, error = %e, "failed to persist funding payment");
                                    }
                                    if let Err(e) = exchange_db::position_repo::PositionRepo::add_funding(
                                        &pool, db_id, pay_amount,
                                    ).await {
                                        tracing::error!(user_id = %uid, error = %e, "failed to update position funding in DB");
                                    }
                                });
                            }
                        }
                    }

                    let _ = exchange.private_tx.send(PrivateEvent::FundingPayment {
                        user_id: payment.user_id.to_string(),
                        symbol: payment.symbol.to_string(),
                        amount: payment.amount,
                        rate: payment.rate,
                    });

                    // NOTIF-8: Push notification for funding payments.
                    if !payment.amount.is_zero() {
                        let direction = if payment.amount > rust_decimal::Decimal::ZERO { "received" } else { "paid" };
                        exchange.send_push_notification(
                            payment.user_id,
                            "Funding Payment",
                            &format!(
                                "You {} {} USDT funding on {}",
                                direction, payment.amount.abs(), payment.symbol
                            ),
                            exchange_admin::NotificationType::FundingPayment,
                            serde_json::json!({
                                "symbol": payment.symbol.as_str(),
                                "amount": payment.amount.to_string(),
                                "rate": payment.rate.to_string(),
                            }),
                        );
                    }
                }

                tracing::info!(
                    symbol = %symbol,
                    rate = %funding_rate,
                    payments = payments.len(),
                    "funding settled"
                );
            }

            // Record settlement time
            exchange.funding_scheduler.lock().unwrap_or_else(|e| e.into_inner()).record_settlement(now);

            exchange.metrics.touch_task_heartbeat("funding_settler");
        }
        })
    })
}

/// Check trigger orders (stop, take-profit, trailing stop) every `interval_ms` milliseconds.
///
/// For each symbol, fetches the last/mark/index price and calls [`TriggerEngine::check_triggers`].
/// Triggered orders are forwarded to the matching engine via [`Exchange::place_order`].
pub fn spawn_trigger_checker(
    exchange: Arc<Exchange>,
    interval_ms: u64,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    supervised_spawn_with_shutdown("trigger_checker", move || {
        let exchange = Arc::clone(&exchange);
        let mut shutdown_rx = shutdown_rx.clone();
        Box::pin(async move {
        let mut ticker = interval(Duration::from_millis(interval_ms));

        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("trigger_checker: shutdown signal received — exiting");
                        return;
                    }
                }
            }

            let now = chrono::Utc::now();
            let symbols: Vec<_> = exchange.perp_configs.load().keys().cloned().collect();

            for symbol in &symbols {
                // last price: order book mid; falls back to index
                let last_price = {
                    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                    match pool.snapshot(symbol, 1) {
                        Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                            (Some((bid, _)), Some((ask, _))) => {
                                Some((*bid + *ask) / rust_decimal::Decimal::TWO)
                            }
                            (Some((bid, _)), None) => Some(*bid),
                            (None, Some((ask, _))) => Some(*ask),
                            (None, None) => None,
                        },
                        Err(_) => None,
                    }
                };

                let mark_price = exchange
                    .get_mark_price(symbol)
                    .map(|mp| mp.price);

                let index_price = {
                    let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                    agg.calculate_index(symbol).ok().map(|ip| ip.price)
                };

                // Need at least one price to evaluate triggers
                let (last, mark, index) = match (last_price, mark_price, index_price) {
                    (Some(l), Some(m), Some(i)) => (l, m, i),
                    (Some(l), Some(m), None) => (l, m, l),
                    (Some(l), None, Some(i)) => (l, i, i),
                    (None, Some(m), Some(i)) => (m, m, i),
                    (Some(l), None, None) => (l, l, l),
                    _ => continue,
                };

                let triggered = {
                    let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
                    te.check_triggers(symbol, last, mark, index, now)
                };

                for result in triggered {
                    let order = &result.trigger_order;
                    tracing::info!(
                        order_id = %order.id,
                        symbol = %order.symbol,
                        trigger_type = ?order.trigger_type,
                        "trigger fired — forwarding to matching engine"
                    );

                    if let Command::PlaceOrder(cmd) = result.command {
                        if let Err(e) = exchange.place_order(cmd) {
                            tracing::error!(
                                order_id = %order.id,
                                error = %e,
                                "failed to place triggered order"
                            );
                        }
                    }

                    // OCO: if this trigger is part of a bracket, cancel the other leg.
                    let other_trigger_id = {
                        use exchange_trigger_service::TriggerOrderType;
                        let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(bracket) = bm.bracket_for_trigger(order.id).cloned() {
                            match order.trigger_type {
                                TriggerOrderType::TakeProfitMarket | TriggerOrderType::TakeProfit => {
                                    bm.on_tp_filled(bracket.id)
                                }
                                TriggerOrderType::StopMarket | TriggerOrderType::StopLimit => {
                                    bm.on_sl_filled(bracket.id)
                                }
                                TriggerOrderType::TrailingStop => {
                                    bm.on_sl_filled(bracket.id)
                                }
                            }
                        } else {
                            None
                        }
                    };

                    if let Some(cancel_id) = other_trigger_id {
                        let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
                        match te.cancel_order(cancel_id) {
                            Ok(_) => tracing::info!(
                                triggered = %order.id,
                                cancelled = %cancel_id,
                                "bracket OCO: cancelled other leg"
                            ),
                            Err(e) => tracing::warn!(
                                triggered = %order.id,
                                cancel_id = %cancel_id,
                                error = %e,
                                "bracket OCO: failed to cancel other leg"
                            ),
                        }
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("trigger_checker");
        }
        })
    })
}

/// Verify ledger integrity and compare in-memory state against DB every 15 minutes.
///
/// Runs continuously as long as a DB pool is available.  When no pool is set
/// (in-memory dev mode) each tick is a no-op.
///
/// Two checks per cycle:
/// 1. **Ledger invariant** — every transaction in `ledger_entries` must sum to zero.
///
/// Periodically snapshot all open futures positions into the WAL.
///
/// On crash recovery, the most recent snapshot is used to restore the
/// PositionManager without needing to reconstruct from individual fills.
///
/// Runs every 60 seconds. Each snapshot is a single WAL entry containing
/// all non-closed positions.
pub fn spawn_position_snapshotter(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("position_snapshotter", move || {
        let ex = Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;

                let positions: Vec<exchange_common::futures::Position> = {
                    let pm = ex.positions.lock();
                    pm.all_positions()
                        .into_iter()
                        .filter(|p| !p.is_closed())
                        .cloned()
                        .collect()
                };

                if positions.is_empty() {
                    continue; // No open positions → skip WAL write
                }

                let cmd = exchange_common::Command::PositionSnapshot {
                    positions: positions.clone(),
                };

                ex.append_to_wal(&cmd);
                tracing::debug!(
                    count = positions.len(),
                    "position_snapshotter: snapshot written to WAL"
                );
            }
        })
    })
}

/// Distribute LP rewards to liquidity mining positions.
///
/// Every 60 seconds, for each pool with `total_fees_earned > 0`, distributes
/// fees pro-rata to LP positions based on their share of the pool. Fees are
/// moved from the pool's `total_fees_earned` into each position's `pending_rewards`.
///
/// Reference: Uniswap v2 fee model — fees auto-compound into reserves.
/// We use a simpler model: fees accrue as USDT rewards, claimable on withdrawal.
pub fn spawn_lp_reward_accrual(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("lp_reward_accrual", move || {
        let ex = Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;

                // Snapshot pool IDs and undistributed fees.
                let pool_snapshots: Vec<(Uuid, Decimal, Decimal)> = {
                    let pools = ex.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner());
                    pools.iter()
                        .filter(|p| p.total_fees_earned > Decimal::ZERO && p.total_lp_shares > Decimal::ZERO)
                        .map(|p| (p.id, p.total_fees_earned, p.total_lp_shares))
                        .collect()
                };

                if pool_snapshots.is_empty() {
                    continue;
                }

                for (pool_id, fees_to_distribute, total_shares) in &pool_snapshots {
                    // Distribute pro-rata to all positions in this pool.
                    let mut distributed = Decimal::ZERO;
                    {
                        let mut positions = ex.liquidity_positions.lock().unwrap_or_else(|e| e.into_inner());
                        for pos in positions.iter_mut().filter(|p| p.pool_id == *pool_id) {
                            let share_fraction = pos.lp_shares / *total_shares;
                            let reward = *fees_to_distribute * share_fraction;
                            pos.pending_rewards += reward;
                            distributed += reward;
                        }
                    }

                    // Deduct distributed fees from the pool.
                    if distributed > Decimal::ZERO {
                        let mut pools = ex.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(pool) = pools.iter_mut().find(|p| p.id == *pool_id) {
                            pool.total_fees_earned -= distributed;
                        }
                        tracing::debug!(
                            pool_id = %pool_id,
                            distributed = %distributed,
                            "LP rewards distributed"
                        );
                    }
                }
            }
        })
    })
}

/// Reconcile in-memory fill count against ledger-persisted fill count.
///
/// Every 300 seconds, compares `fills_processed` (incremented on every in-memory
/// fill) with `fills_ledger_written` (incremented after each successful DB ledger
/// write). A persistent delta indicates potential data loss in the write-behind path.
///
/// Reference: Financial audit trail integrity — all balance-affecting events must
/// be recorded in the double-entry ledger for regulatory compliance.
pub fn spawn_ledger_reconciliation(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("ledger_reconciliation", move || {
        let ex = Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(300));
            let mut prev_delta: u64 = 0;

            loop {
                ticker.tick().await;

                let processed = ex.fills_processed.load(std::sync::atomic::Ordering::Relaxed);
                let written = ex.fills_ledger_written.load(std::sync::atomic::Ordering::Relaxed);
                let delta = processed.saturating_sub(written);

                if delta == 0 {
                    tracing::debug!(
                        processed = processed,
                        written = written,
                        "ledger reconciliation: all fills persisted"
                    );
                } else if delta == prev_delta && delta > 0 {
                    // Same delta for two cycles — potential data loss.
                    tracing::error!(
                        processed = processed,
                        written = written,
                        delta = delta,
                        "LEDGER RECONCILIATION: persistent delta — fills may not be persisted!"
                    );
                } else {
                    // Delta present but changed — write-behind is still catching up.
                    tracing::warn!(
                        processed = processed,
                        written = written,
                        delta = delta,
                        "ledger reconciliation: write-behind catching up"
                    );
                }

                prev_delta = delta;
            }
        })
    })
}

/// 2. A simple heartbeat log confirming the cycle completed.
///
/// On violation the error is logged at `ERROR` level so it surfaces in
/// monitoring dashboards; the job continues running so subsequent cycles can
/// report whether the violation was transient.
pub fn spawn_reconciliation_job(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("reconciliation_job", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(900)); // 15 minutes

        loop {
            ticker.tick().await;

            let pool = match exchange.db_pool.get().cloned() {
                Some(p) => p,
                None => {
                    // No DB configured — skip silently.
                    exchange.metrics.touch_task_heartbeat("reconciliation_job");
                    continue;
                }
            };

            // 1. Ledger double-entry invariant check.
            match exchange_db::ledger::LedgerService::verify_invariant(&pool).await {
                Ok(true) => tracing::info!("reconciliation: ledger invariant OK"),
                Ok(false) => tracing::error!("RECONCILIATION ALERT: ledger double-entry invariant VIOLATED — transactions do not sum to zero"),
                Err(e) => tracing::error!(error = %e, "reconciliation: ledger invariant check failed"),
            }

            tracing::info!("reconciliation job completed");
            exchange.metrics.touch_task_heartbeat("reconciliation_job");
        }
        })
    })
}

/// Poll withdrawal status every `interval_ms` milliseconds.
///
/// Delegates to [`WithdrawalManager::poll_status`] which checks all `Processing`
/// withdrawals against the custody provider and transitions them to `Completed`
/// or `Failed`. On failure the caller is responsible for any refund logic; the
/// manager records the state transition only.
pub fn spawn_withdrawal_poller(exchange: Arc<Exchange>, interval_ms: u64) -> JoinHandle<()> {
    supervised_spawn("withdrawal_poller", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_millis(interval_ms));

        loop {
            ticker.tick().await;

            // Collect processing withdrawal IDs (short lock).
            let processing: Vec<(uuid::Uuid, String)> = {
                let wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
                wm.processing_withdrawals()
            };

            // Check each with custody provider (async, no lock).
            for (wid, provider_tx_id) in processing {
                match exchange.custody_provider.check_withdrawal_status(&provider_tx_id).await {
                    Ok(exchange_custody::ProviderTxStatus::Completed { tx_hash }) => {
                        let mut wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
                        let _ = wm.on_confirmed(&wid, tx_hash);
                    }
                    Ok(exchange_custody::ProviderTxStatus::Failed { reason }) => {
                        let mut wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
                        let _ = wm.on_failed(&wid, reason);
                    }
                    Ok(exchange_custody::ProviderTxStatus::Confirming { .. }) | Ok(exchange_custody::ProviderTxStatus::Pending) => {}
                    Err(e) => {
                        tracing::warn!(withdrawal_id = %wid, "Failed to check withdrawal status: {}", e);
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("withdrawal_poller");
        }
        })
    })
}

/// Cancel all resting GTD orders whose `expire_at` timestamp has passed.
///
/// Runs every second. Collects all expired order IDs from the registry in one
/// lock acquisition, then cancels each via the normal [`Exchange::cancel_order`]
/// path so that balance unlocks, history, and WS events are all handled
/// consistently.
pub fn spawn_gtd_expiry_checker(
    exchange: Arc<Exchange>,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    supervised_spawn_with_shutdown("gtd_expiry_checker", move || {
        let exchange = Arc::clone(&exchange);
        let mut shutdown_rx = shutdown_rx.clone();
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("gtd_expiry_checker: shutdown signal received — exiting");
                        return;
                    }
                }
            }

            let now = chrono::Utc::now();

            // Collect expired order IDs without holding the lock during cancellation.
            let expired: Vec<(Uuid, exchange_common::Symbol)> = {
                let registry = exchange.gtd_orders.lock().unwrap_or_else(|e| e.into_inner());
                registry
                    .iter()
                    .filter(|(_, (_, expire_at))| *expire_at <= now)
                    .map(|(oid, (sym, _))| (*oid, sym.clone()))
                    .collect()
            };

            for (order_id, symbol) in expired {
                tracing::debug!(
                    order_id = %order_id,
                    symbol = %symbol,
                    "GTD order expired — cancelling"
                );

                let cmd = CancelOrderCommand {
                    order_id,
                    user_id: {
                        // Resolve user_id from pending_orders for the WAL record.
                        exchange
                            .pending_orders
                            .get(&order_id)
                            .map(|info| info.user_id)
                            .unwrap_or_else(Uuid::nil)
                    },
                    symbol: symbol.clone(),
                    timestamp: now,
                };

                if let Err(e) = exchange.cancel_order(cmd) {
                    // The order may have already been filled or cancelled between
                    // collection and this point — that is safe to ignore.
                    tracing::debug!(
                        order_id = %order_id,
                        error = %e,
                        "GTD expiry cancel failed (order may already be gone)"
                    );
                }
            }
            exchange.metrics.touch_task_heartbeat("gtd_expiry_checker");
        }
        })
    })
}

/// Submit pending algo order slices every second.
///
/// On each tick, checks all active TWAP and VWAP executions for slices that
/// are due (scheduled_at <= now). For each due slice, calls
/// `generate_slice_order` to build an [`AlgoOrder`], constructs a
/// [`PlaceOrderCommand`], and forwards it to the matching engine via
/// [`Exchange::place_order`].
///
/// After a successful submission the slice is marked `Submitted`. When the
/// price limit is breached (checked once per execution per tick) the execution
/// is halted — no further slices are submitted for that algo. Completed or
/// cancelled executions are pruned from the map so memory does not grow
/// unboundedly.
pub fn spawn_algo_executor(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("algo_executor", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(1));

        loop {
            ticker.tick().await;

            let now = chrono::Utc::now();

            // --- TWAP ---
            let twap_ids: Vec<Uuid> = {
                let twap = exchange
                    .twap_executions
                    .lock().unwrap_or_else(|e| e.into_inner());
                twap.keys().copied().collect()
            };

            for algo_id in twap_ids {
                // Resolve current mid-price from the order book.
                let symbol = {
                    let twap = exchange
                        .twap_executions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    match twap.get(&algo_id).map(|e| e.config.symbol.clone()) {
                        Some(s) => s,
                        None => continue,
                    }
                };

                let mid_price = {
                    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                    match pool.snapshot(&symbol, 1) {
                        Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                            (Some((bid, _)), Some((ask, _))) => (*bid + *ask) / Decimal::TWO,
                            (Some((bid, _)), None) => *bid,
                            (None, Some((ask, _))) => *ask,
                            (None, None) => {
                                tracing::debug!(algo_id = %algo_id, "TWAP: no book depth, skipping tick");
                                continue;
                            }
                        },
                        Err(_) => continue,
                    }
                };

                // Check price limit and find next due slice index.
                let maybe_slice = {
                    let mut twap = exchange
                        .twap_executions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    let exec = match twap.get_mut(&algo_id) {
                        Some(e) => e,
                        None => continue,
                    };

                    if !exec.check_price_limit(mid_price) {
                        tracing::warn!(algo_id = %algo_id, "TWAP price limit breached — execution halted");
                        continue;
                    }

                    exec.next_due_slice(now)
                        .map(|s| (s.index, exec.generate_slice_order(s.index, mid_price)))
                };

                if let Some((idx, Some(order))) = maybe_slice {
                    // Round slice qty to product lot_size to pass order validation.
                    let rounded_qty = if let Some(prod) = exchange.accounts.get_product(&order.symbol) {
                        let lot = prod.lot_size;
                        if lot > Decimal::ZERO { (order.qty / lot).floor() * lot } else { order.qty }
                    } else { order.qty };
                    if rounded_qty <= Decimal::ZERO { continue; }
                    let order_id = Uuid::new_v4();
                    let cmd = PlaceOrderCommand {
                        order_id,
                        user_id: order.user_id,
                        symbol: order.symbol.clone(),
                        side: order.side,
                        order_type: order.order_type,
                        time_in_force: TimeInForce::Ioc,
                        price: order.price.unwrap_or(Decimal::ZERO),
                        qty: rounded_qty,
                        reduce_only: false,
                        expire_at: None,
                        timestamp: now,
                        position_idx: 0,
                        iceberg_qty: None,
                        order_link_id: None,
                        slippage_tolerance: None,
                        close_position: false,
                        stp_mode: StpMode::default(),
                        request_expire_time: None,
                    };

                    match exchange.place_order(cmd) {
                        Ok(_) => {
                            let mut twap = exchange
                                .twap_executions
                                .lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(exec) = twap.get_mut(&algo_id) {
                                exec.mark_submitted(idx, order_id);
                            }
                            tracing::debug!(
                                algo_id = %algo_id,
                                slice_index = idx,
                                order_id = %order_id,
                                "TWAP slice submitted"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                algo_id = %algo_id,
                                slice_index = idx,
                                error = %e,
                                "TWAP slice placement failed"
                            );
                        }
                    }
                }

                // Prune terminal executions.
                {
                    let mut twap = exchange
                        .twap_executions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(exec) = twap.get(&algo_id) {
                        if matches!(
                            exec.status,
                            exchange_algo_execution::AlgoStatus::Completed
                                | exchange_algo_execution::AlgoStatus::Cancelled
                                | exchange_algo_execution::AlgoStatus::PriceLimitBreached
                        ) {
                            twap.remove(&algo_id);
                            tracing::info!(algo_id = %algo_id, "TWAP execution removed (terminal)");
                        }
                    }
                }
            }

            // --- VWAP ---
            let vwap_ids: Vec<Uuid> = {
                let vwap = exchange
                    .vwap_executions
                    .lock().unwrap_or_else(|e| e.into_inner());
                vwap.keys().copied().collect()
            };

            for algo_id in vwap_ids {
                let symbol = {
                    let vwap = exchange
                        .vwap_executions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    match vwap.get(&algo_id).map(|e| e.config.symbol.clone()) {
                        Some(s) => s,
                        None => continue,
                    }
                };

                let mid_price = {
                    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                    match pool.snapshot(&symbol, 1) {
                        Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                            (Some((bid, _)), Some((ask, _))) => (*bid + *ask) / Decimal::TWO,
                            (Some((bid, _)), None) => *bid,
                            (None, Some((ask, _))) => *ask,
                            (None, None) => {
                                tracing::debug!(algo_id = %algo_id, "VWAP: no book depth, skipping tick");
                                continue;
                            }
                        },
                        Err(_) => continue,
                    }
                };

                let maybe_slice = {
                    let mut vwap = exchange
                        .vwap_executions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    let exec = match vwap.get_mut(&algo_id) {
                        Some(e) => e,
                        None => continue,
                    };

                    if !exec.check_price_limit(mid_price) {
                        tracing::warn!(algo_id = %algo_id, "VWAP price limit breached — execution halted");
                        continue;
                    }

                    exec.next_due_slice(now)
                        .map(|s| (s.index, exec.generate_slice_order(s.index, mid_price)))
                };

                if let Some((idx, Some(order))) = maybe_slice {
                    // Round slice qty to product lot_size to pass order validation.
                    let rounded_qty = if let Some(prod) = exchange.accounts.get_product(&order.symbol) {
                        let lot = prod.lot_size;
                        if lot > Decimal::ZERO { (order.qty / lot).floor() * lot } else { order.qty }
                    } else { order.qty };
                    if rounded_qty <= Decimal::ZERO { continue; }
                    let order_id = Uuid::new_v4();
                    let cmd = PlaceOrderCommand {
                        order_id,
                        user_id: order.user_id,
                        symbol: order.symbol.clone(),
                        side: order.side,
                        order_type: order.order_type,
                        time_in_force: TimeInForce::Ioc,
                        price: order.price.unwrap_or(Decimal::ZERO),
                        qty: rounded_qty,
                        reduce_only: false,
                        expire_at: None,
                        timestamp: now,
                        position_idx: 0,
                        iceberg_qty: None,
                        order_link_id: None,
                        slippage_tolerance: None,
                        close_position: false,
                        stp_mode: StpMode::default(),
                        request_expire_time: None,
                    };

                    match exchange.place_order(cmd) {
                        Ok(_) => {
                            let mut vwap = exchange
                                .vwap_executions
                                .lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(exec) = vwap.get_mut(&algo_id) {
                                exec.mark_submitted(idx, order_id);
                            }
                            tracing::debug!(
                                algo_id = %algo_id,
                                slice_index = idx,
                                order_id = %order_id,
                                "VWAP slice submitted"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                algo_id = %algo_id,
                                slice_index = idx,
                                error = %e,
                                "VWAP slice placement failed"
                            );
                        }
                    }
                }

                // Prune terminal executions.
                {
                    let mut vwap = exchange
                        .vwap_executions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(exec) = vwap.get(&algo_id) {
                        if matches!(
                            exec.status,
                            exchange_algo_execution::AlgoStatus::Completed
                                | exchange_algo_execution::AlgoStatus::Cancelled
                                | exchange_algo_execution::AlgoStatus::PriceLimitBreached
                        ) {
                            vwap.remove(&algo_id);
                            tracing::info!(algo_id = %algo_id, "VWAP execution removed (terminal)");
                        }
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("algo_executor");
        }
        })
    })
}

/// Cancel all open orders for users whose dead man's switch deadline has passed.
///
/// Checks every second. For each expired entry, collects the user's pending
/// orders (from `pending_orders`), cancels them via the normal
/// [`Exchange::cancel_order`] path, and removes the DMS entry so it does not
/// fire again.
pub fn spawn_dead_mans_switch(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("dead_mans_switch", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(1));

        loop {
            ticker.tick().await;

            let now = chrono::Utc::now();

            // Collect expired user IDs without holding the DMS lock during cancellation.
            let expired_users: Vec<Uuid> = {
                let dms = exchange.dead_mans_switch.lock().unwrap_or_else(|e| e.into_inner());
                dms.iter()
                    .filter(|(_, deadline)| **deadline <= now)
                    .map(|(uid, _)| *uid)
                    .collect()
            };

            for user_id in expired_users {
                tracing::warn!(
                    user_id = %user_id,
                    "dead man's switch triggered — cancelling all open orders"
                );

                // Remove the entry first so a slow cancellation loop does not
                // fire the switch again on the next tick.
                exchange
                    .dead_mans_switch
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .remove(&user_id);

                // Collect (order_id, symbol) pairs for this user.
                let orders: Vec<(Uuid, exchange_common::Symbol)> = exchange
                    .pending_orders
                    .iter()
                    .filter(|r| r.user_id == user_id)
                    .map(|r| (*r.key(), r.symbol.clone()))
                    .collect();

                for (order_id, symbol) in orders {
                    let cmd = CancelOrderCommand {
                        order_id,
                        user_id,
                        symbol: symbol.clone(),
                        timestamp: now,
                    };

                    if let Err(e) = exchange.cancel_order(cmd) {
                        // Already filled or cancelled between collection and now — safe to ignore.
                        tracing::debug!(
                            user_id = %user_id,
                            order_id = %order_id,
                            error = %e,
                            "DMS cancel failed (order may already be gone)"
                        );
                    } else {
                        tracing::info!(
                            user_id = %user_id,
                            order_id = %order_id,
                            symbol = %symbol,
                            "DMS: order cancelled"
                        );
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("dead_mans_switch");
        }
        })
    })
}

/// Poll pending deposits and log them for admin review.
///
/// Deposits are NOT auto-credited here. Real crediting happens via one of two
/// paths:
///   1. The webhook handler (`webhook_deposit`) called by the custody provider
///      when a blockchain deposit is confirmed.
///   2. Admin manual approval via `POST /admin/deposits/{id}/credit`.
///
/// Auto-crediting without blockchain confirmation is a direct financial exploit
/// (an attacker could create a deposit record without ever sending funds).
///
/// Runs every `interval_ms` milliseconds.
pub fn spawn_deposit_poller(exchange: Arc<Exchange>, interval_ms: u64) -> JoinHandle<()> {
    supervised_spawn("deposit_poller", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_millis(interval_ms));

        loop {
            ticker.tick().await;

            // Snapshot pending deposits under a short lock.
            let pending: Vec<_> = {
                let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
                dm.pending_deposits().into_iter().cloned().collect()
            };

            // Phase 1: Log pending deposits for admin review.
            // Real crediting happens via webhook (rest.rs webhook_deposit)
            // when custody provider confirms blockchain receipt.
            if !pending.is_empty() {
                tracing::info!(
                    pending_count = pending.len(),
                    "deposit poller: {} deposits awaiting confirmation",
                    pending.len()
                );
            }
            exchange.metrics.touch_task_heartbeat("deposit_poller");
        }
        })
    })
}

/// Execute active trading bots every `interval_ms` milliseconds.
///
/// **DCA bots**: calls [`DcaBot::should_execute`] with the current mid-price;
/// when it returns `true`, calls [`DcaBot::generate_order`] and places a
/// market buy via the matching engine.
///
/// **Grid bots**: on first tick for a newly-created bot (no live orders yet),
/// calls [`GridBot::generate_initial_orders`] to place the resting limit grid.
/// Reactive fill handling (calling [`GridBot::on_fill`]) happens in the fill
/// event path, not here.
///
/// Completed or stopped bots are pruned from memory each tick.
async fn run_bot_executor(exchange: Arc<Exchange>, interval_ms: u64) {
    let mut ticker = interval(Duration::from_millis(interval_ms));

        loop {
            ticker.tick().await;

            let now = Utc::now();

            // --- DCA bots ---
            let dca_ids: Vec<Uuid> = {
                let bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
                bots.iter().map(|b| b.id).collect()
            };

            for bot_id in dca_ids {
                // Resolve symbol and check whether we should execute.
                let (symbol, user_id, should_exec, current_price) = {
                    let bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(bot) = bots.iter().find(|b| b.id == bot_id) else {
                        continue;
                    };

                    // Get the current mid-price from the order book.
                    let mid_price = {
                        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                        match pool.snapshot(&bot.config.symbol, 1) {
                            Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                                (Some((bid, _)), Some((ask, _))) => (*bid + *ask) / Decimal::TWO,
                                (Some((bid, _)), None) => *bid,
                                (None, Some((ask, _))) => *ask,
                                (None, None) => {
                                    tracing::debug!(bot_id = %bot_id, "DCA: no book depth, skipping tick");
                                    continue;
                                }
                            },
                            Err(_) => continue,
                        }
                    };

                    (
                        bot.config.symbol.clone(),
                        bot.config.user_id,
                        bot.should_execute(now, mid_price),
                        mid_price,
                    )
                };

                if !should_exec {
                    continue;
                }

                // Generate the order, mutating bot state.
                let bot_order = {
                    let mut bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(bot) = bots.iter_mut().find(|b| b.id == bot_id) else {
                        continue;
                    };
                    bot.generate_order(current_price)
                };

                let cmd = PlaceOrderCommand {
                    order_id: bot_order.order_id,
                    user_id,
                    symbol: symbol.clone(),
                    side: bot_order.side,
                    order_type: bot_order.order_type,
                    time_in_force: TimeInForce::Ioc,
                    price: Decimal::ZERO,
                    qty: bot_order.qty,
                    reduce_only: false,
                    expire_at: None,
                    timestamp: now,
                    position_idx: 0,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: StpMode::default(),
                    request_expire_time: None,
                };

                match exchange.place_order(cmd) {
                    Ok(_) => {
                        tracing::info!(
                            bot_id = %bot_id,
                            symbol = %symbol,
                            qty = %bot_order.qty,
                            price = %current_price,
                            "DCA bot order placed"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            bot_id = %bot_id,
                            symbol = %symbol,
                            error = %e,
                            "DCA bot order failed"
                        );
                    }
                }
            }

            // Prune completed DCA bots.
            {
                let mut bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
                bots.retain(|b| !b.is_complete());
            }

            // --- Grid bots ---
            let grid_ids: Vec<Uuid> = {
                let bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
                bots.iter().map(|b| b.id).collect()
            };

            for bot_id in grid_ids {
                // A grid bot needs initial orders placed when it has no live orders yet.
                let needs_init = {
                    let bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(bot) = bots.iter().find(|b| b.id == bot_id) else {
                        continue;
                    };
                    // Bot is fresh if none of its grid lines have any assigned order IDs.
                    bot.grid_lines
                        .iter()
                        .all(|l| l.buy_order_id.is_none() && l.sell_order_id.is_none())
                };

                if !needs_init {
                    continue;
                }

                let (symbol, user_id, init_orders) = {
                    // Get the current mid-price from the order book.
                    let mid_price = {
                        let sym = {
                            let bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
                            bots.iter().find(|b| b.id == bot_id).map(|b| b.config.symbol.clone())
                        };
                        let Some(sym) = sym else { continue };

                        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                        match pool.snapshot(&sym, 1) {
                            Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                                (Some((bid, _)), Some((ask, _))) => (*bid + *ask) / Decimal::TWO,
                                (Some((bid, _)), None) => *bid,
                                (None, Some((ask, _))) => *ask,
                                (None, None) => {
                                    tracing::debug!(bot_id = %bot_id, "Grid: no book depth, skipping tick");
                                    continue;
                                }
                            },
                            Err(_) => continue,
                        }
                    };

                    let mut bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(bot) = bots.iter_mut().find(|b| b.id == bot_id) else {
                        continue;
                    };
                    let orders = bot.generate_initial_orders(mid_price);
                    (bot.config.symbol.clone(), bot.config.user_id, orders)
                };

                for bot_order in init_orders {
                    let cmd = PlaceOrderCommand {
                        order_id: bot_order.order_id,
                        user_id,
                        symbol: symbol.clone(),
                        side: bot_order.side,
                        order_type: bot_order.order_type,
                        time_in_force: TimeInForce::Gtc,
                        price: bot_order.price,
                        qty: bot_order.qty,
                        reduce_only: false,
                        expire_at: None,
                        timestamp: now,
                        position_idx: 0,
                        iceberg_qty: None,
                        order_link_id: None,
                        slippage_tolerance: None,
                        close_position: false,
                        stp_mode: StpMode::default(),
                        request_expire_time: None,
                    };

                    if let Err(e) = exchange.place_order(cmd) {
                        tracing::error!(
                            bot_id = %bot_id,
                            symbol = %symbol,
                            side = ?bot_order.side,
                            price = %bot_order.price,
                            error = %e,
                            "grid bot initial order failed"
                        );
                    }
                }

                tracing::info!(bot_id = %bot_id, symbol = %symbol, "grid bot initial orders placed");
            }

            // Prune stopped grid bots.
            {
                let mut bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
                bots.retain(|b| b.status != BotStatus::Stopped);
            }
        }
}

/// Periodically re-screen all users against sanctions lists.
///
/// Phase 1: logs a summary. Phase 2: iterates all users, calls
/// `sanctions.full_screen(name, addresses)` for each, and flags/blocks any match.
pub fn spawn_sanctions_rescreener(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("sanctions_rescreener", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        // Offset the first tick by 6 hours so this does not run on startup.
        let mut ticker = interval(Duration::from_secs(6 * 3600));
        ticker.tick().await; // consume the immediate first tick
        loop {
            ticker.tick().await;
            tracing::info!("sanctions_rescreener: starting periodic check");

            // Phase 1: count registered users as a proxy for work done.
            let user_count = exchange.accounts.user_count();

            tracing::info!(
                users = user_count,
                "sanctions_rescreener: complete (phase 1 — no per-user screening yet)"
            );
            exchange.metrics.touch_task_heartbeat("sanctions_rescreener");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// M21-2: OFAC SDN feed ingestion
// ---------------------------------------------------------------------------

/// Periodically fetch the OFAC SDN list and update the sanctions screener.
///
/// Phase 1: logs that the feed would be fetched (no actual HTTP call).
/// Phase 2: parse SDN XML from treasury.gov, bulk_load into SanctionsScreener.
/// On parse failure, the previous list is retained (never silently emptied).
/// Default OFAC SDN CSV URL (names).
const OFAC_SDN_CSV_URL: &str = "https://www.treasury.gov/ofac/downloads/sdn.csv";
/// Default OFAC address CSV URL (includes crypto addresses in remarks).
const OFAC_ADD_CSV_URL: &str = "https://www.treasury.gov/ofac/downloads/add.csv";

pub fn spawn_ofac_feed_updater(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("ofac_feed_updater", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let sdn_url = std::env::var("OFAC_SDN_URL").unwrap_or_else(|_| OFAC_SDN_CSV_URL.to_string());
        let add_url = std::env::var("OFAC_ADD_URL").unwrap_or_else(|_| OFAC_ADD_CSV_URL.to_string());

        // Initial fetch after 60 seconds, then every 24 hours.
        tokio::time::sleep(Duration::from_secs(60)).await;

        loop {
            tracing::info!("ofac_feed_updater: fetching OFAC SDN list");

            match fetch_ofac_lists(&sdn_url, &add_url).await {
                Ok((names, addresses)) => {
                    let name_count = names.len();
                    let addr_count = addresses.len();

                    let mut screener = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
                    screener.load_ofac_names(names);
                    screener.load_sanctioned_addresses(addresses);

                    tracing::info!(
                        names = name_count,
                        addresses = addr_count,
                        total = screener.name_count() + screener.address_count(),
                        "ofac_feed_updater: sanctions list updated from treasury.gov"
                    );

                    // P3-3: Track last successful refresh for staleness detection.
                    exchange.last_ofac_refresh_ms.store(
                        chrono::Utc::now().timestamp_millis() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "ofac_feed_updater: failed to fetch OFAC SDN list — retaining previous list"
                    );
                }
            }

            // P3-3: Check if the list is stale (last refresh >48h ago).
            let last_ms = exchange.last_ofac_refresh_ms.load(std::sync::atomic::Ordering::Relaxed);
            if last_ms > 0 {
                let age_hours = (chrono::Utc::now().timestamp_millis() as u64 - last_ms) / 3_600_000;
                if age_hours > 48 {
                    tracing::error!(
                        age_hours,
                        "ofac_feed_updater: SANCTIONS LIST STALE — last refreshed {}h ago (>48h threshold)",
                        age_hours,
                    );
                }
            }

            exchange.metrics.touch_task_heartbeat("ofac_feed_updater");
            // Sleep 24 hours before next fetch.
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
        })
    })
}

/// Fetch and parse OFAC SDN CSV files.
///
/// Returns (names, crypto_addresses).
/// Reference: https://www.treasury.gov/resource-center/sanctions/SDN-List/Pages/default.aspx
async fn fetch_ofac_lists(sdn_url: &str, add_url: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    // 1. Fetch SDN names.
    let sdn_body = client.get(sdn_url).send().await
        .map_err(|e| format!("SDN fetch error: {e}"))?
        .text().await
        .map_err(|e| format!("SDN read error: {e}"))?;

    let mut names = Vec::new();
    for line in sdn_body.lines() {
        // SDN CSV format: ent_num, "SDN_Name", "SDN_Type", "Program", ...
        // Fields are in quotes, comma-separated. Column 2 (index 1) is the name.
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 2 {
            let name = fields[1].trim().trim_matches('"');
            if !name.is_empty() && name != "SDN_Name" {
                names.push(name.to_string());
            }
        }
    }

    // 2. Fetch address file — look for crypto addresses.
    let add_body = client.get(add_url).send().await
        .map_err(|e| format!("ADD fetch error: {e}"))?
        .text().await
        .map_err(|e| format!("ADD read error: {e}"))?;

    let mut addresses = Vec::new();
    for line in add_body.lines() {
        // ADD CSV: ent_num, Add_Num, Address, CityStateProvincePostalCode, Country, Add_Remarks
        // Crypto addresses appear in the Address field or Remarks field.
        // Look for typical crypto address patterns.
        let lower = line.to_lowercase();
        if lower.contains("digital currency address") || lower.contains("xbt") || lower.contains("eth") {
            // Extract the address from field 3 (index 2).
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() >= 3 {
                let addr = fields[2].trim().trim_matches('"');
                if !addr.is_empty() && (addr.starts_with("0x") || addr.starts_with("bc1") ||
                    addr.starts_with("1") || addr.starts_with("3") || addr.starts_with("T")) {
                    addresses.push(addr.to_string());
                }
            }
        }
    }

    if names.is_empty() {
        return Err("OFAC SDN list parsed 0 names — possible format change".to_string());
    }

    Ok((names, addresses))
}

/// Monitor trading activity every 5 minutes for AML velocity signals.
///
/// Phase 1 checks:
/// - Users placing more than 100 orders in the last 24 hours are flagged at
///   `WARN` level so compliance staff can investigate manually.
/// - A heartbeat debug log confirms each monitoring cycle completed.
///
/// Future phases will add dormant-account reactivation detection and
/// automated risk score updates.
pub fn spawn_continuous_aml_monitor(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("continuous_aml_monitor", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(300)); // every 5 minutes

        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Count orders per user in the last 24 hours.
            let per_user: std::collections::HashMap<uuid::Uuid, u32> = {
                let history = exchange
                    .order_history
                    .lock().unwrap_or_else(|e| e.into_inner());
                let cutoff = now - chrono::Duration::hours(24);
                let mut counts: std::collections::HashMap<uuid::Uuid, u32> =
                    std::collections::HashMap::new();
                for order in history.iter() {
                    if order.created_at >= cutoff {
                        *counts.entry(order.user_id).or_default() += 1;
                    }
                }
                counts
            };

            // Flag users with unusually high trading velocity.
            for (user_id, count) in &per_user {
                if *count > 100 {
                    tracing::warn!(
                        user_id = %user_id,
                        order_count = count,
                        "continuous AML: high trading velocity detected"
                    );
                }
            }

            tracing::debug!(
                users_checked = per_user.len(),
                "continuous AML monitoring cycle complete"
            );
            exchange.metrics.touch_task_heartbeat("continuous_aml_monitor");
        }
        })
    })
}

/// Background task: check price alerts against current market prices.
///
/// Runs every 5 seconds. When a price alert triggers, marks it as triggered and
/// logs the event (email integration via the email sender if configured).
pub fn spawn_price_alert_checker(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("price_alert_checker", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;

            // Snapshot current prices from the market data service.
            let tickers: std::collections::HashMap<String, rust_decimal::Decimal> = exchange
                .market_data
                .all_tickers()
                .into_iter()
                .map(|t| (t.symbol.to_string(), t.last_price))
                .collect();

            if tickers.is_empty() {
                continue;
            }

            let mut triggered_ids = Vec::new();

            {
                let alerts = exchange
                    .price_alerts
                    .lock().unwrap_or_else(|e| e.into_inner());
                for alert in alerts.iter() {
                    if alert.triggered {
                        continue;
                    }
                    if let Some(&current_price) = tickers.get(&alert.symbol) {
                        let should_trigger = match alert.condition.as_str() {
                            "above" => current_price >= alert.price,
                            "below" => current_price <= alert.price,
                            _ => false,
                        };
                        if should_trigger {
                            triggered_ids.push((alert.id, alert.user_id, alert.symbol.clone(), alert.condition.clone(), alert.price, current_price));
                        }
                    }
                }
            }

            // Mark triggered alerts.
            if !triggered_ids.is_empty() {
                let mut alerts = exchange
                    .price_alerts
                    .lock().unwrap_or_else(|e| e.into_inner());
                for (id, _, _, _, _, _) in &triggered_ids {
                    if let Some(a) = alerts.iter_mut().find(|a| a.id == *id) {
                        a.triggered = true;
                    }
                }
            }

            for (id, user_id, symbol, condition, target, current) in &triggered_ids {
                tracing::info!(
                    alert_id = %id,
                    user_id = %user_id,
                    symbol = %symbol,
                    condition = %condition,
                    target_price = %target,
                    current_price = %current,
                    "price alert triggered"
                );

                // NOTIF-7: Push notification for price alerts.
                exchange.send_push_notification(
                    *user_id,
                    "Price Alert",
                    &format!(
                        "{symbol} is now {condition} your target of {target} (current: {current})",
                    ),
                    exchange_admin::NotificationType::PriceAlert,
                    serde_json::json!({
                        "alert_id": id.to_string(),
                        "symbol": symbol,
                        "condition": condition,
                        "target_price": target.to_string(),
                        "current_price": current.to_string(),
                    }),
                );
            }
            exchange.metrics.touch_task_heartbeat("price_alert_checker");
        }
        })
    })
}

/// Background task: expire old withdrawal confirmation tokens (> 30 minutes).
pub fn spawn_withdrawal_confirmation_expiry(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("withdrawal_confirmation_expiry", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();
            let mut confirmations = exchange
                .withdrawal_confirmations
                .lock().unwrap_or_else(|e| e.into_inner());
            confirmations.retain(|_, v| v.expires_at > now);
            exchange.metrics.touch_task_heartbeat("withdrawal_confirmation_expiry");
        }
        })
    })
}

/// Log a snapshot of every insurance fund balance once per hour.
///
/// Provides a continuous time-series record for auditing and monitoring dashboards.
pub fn spawn_insurance_fund_snapshotter(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("insurance_fund_snapshotter", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        // Offset by 1 minute so the snapshot does not race with other hourly tasks.
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut ticker = interval(Duration::from_secs(3600));
        loop {
            ticker.tick().await;

            let symbols_and_balances: Vec<(exchange_common::types::Symbol, Decimal)> = {
                let funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
                funds.iter().map(|(s, f)| (s.clone(), f.balance())).collect()
            };

            for (symbol, balance) in &symbols_and_balances {
                exchange.record_insurance_fund_event(
                    symbol,
                    Decimal::ZERO,
                    *balance,
                    "hourly_snapshot",
                );
            }

            tracing::info!(
                symbols = symbols_and_balances.len(),
                "insurance fund hourly snapshot recorded"
            );
            exchange.metrics.touch_task_heartbeat("insurance_fund_snapshotter");
        }
        })
    })
}

/// Evaluate market maker SLA compliance every `interval_secs` seconds.
///
/// For each registered active MM, evaluates the SLA monitor for every symbol
/// they have been seen on. Writes results back to `program.reports` and
/// updates penalty levels on the `MarketMakerRecord`:
///
/// * `Warning`       → penalty_level = 1
/// * `ReduceRebates` → penalty_level = 2
/// * `SuspendMm`     → is_active = false, penalty_level = 3
///
/// Feed SLA snapshots to market maker monitors by reading the order book.
///
/// Without this, all MMs are auto-suspended because the SLA evaluator sees zero snapshots.
/// Runs every 60 seconds, checking which registered MMs have orders near the BBO.
pub fn spawn_mm_snapshot_producer(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("mm_snapshot_producer", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            let (active_mm_ids, symbols) = {
                let program = exchange.market_maker_program.lock().unwrap_or_else(|e| e.into_inner());
                let ids: Vec<Uuid> = program.records.iter()
                    .filter(|(_, r)| r.is_active)
                    .map(|(id, _)| *id)
                    .collect();
                let syms: Vec<exchange_common::Symbol> = program.monitors.keys().cloned().collect();
                (ids, syms)
            };

            if active_mm_ids.is_empty() { continue; }

            for symbol in &symbols {
                // Read order book snapshot.
                let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                let snap = match pool.snapshot(symbol, 20) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                drop(pool);

                let best_bid = snap.bids.first().map(|(p, _)| *p).unwrap_or(Decimal::ZERO);
                let best_ask = snap.asks.first().map(|(p, _)| *p).unwrap_or(Decimal::ZERO);
                let spread = if best_bid > Decimal::ZERO && best_ask > Decimal::ZERO {
                    (best_ask - best_bid) / best_ask
                } else {
                    Decimal::ONE
                };

                // For each active MM, check if they have orders near BBO.
                for mm_id in &active_mm_ids {
                    let has_bid = exchange.pending_orders.iter().any(|r| r.user_id == *mm_id && r.symbol == *symbol && r.side == exchange_common::Side::Buy);
                    let has_ask = exchange.pending_orders.iter().any(|r| r.user_id == *mm_id && r.symbol == *symbol && r.side == exchange_common::Side::Sell);

                    let is_present = has_bid || has_ask;
                    let spread_bps = if is_present && best_bid > Decimal::ZERO {
                        Some(spread * Decimal::new(10_000, 0)) // convert ratio to bps
                    } else {
                        None
                    };
                    let bid_depth_quote = if has_bid {
                        Some(snap.bids.iter().map(|(p, q)| *p * *q).sum())
                    } else {
                        None
                    };
                    let ask_depth_quote = if has_ask {
                        Some(snap.asks.iter().map(|(p, q)| *p * *q).sum())
                    } else {
                        None
                    };
                    let mm_snap = exchange_market_maker::sla::MmSnapshot {
                        mm_id: *mm_id,
                        timestamp: now,
                        is_present,
                        spread_bps,
                        bid_depth_quote,
                        ask_depth_quote,
                        refill_secs: None, // TODO: track time since last quote refresh
                    };

                    let mut program = exchange.market_maker_program.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(monitor) = program.monitors.get_mut(symbol) {
                        let _ = monitor.record_snapshot(mm_snap);
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("mm_snapshot_producer");
        }
        })
    })
}

pub fn spawn_mm_sla_evaluator(exchange: Arc<Exchange>, interval_secs: u64) -> JoinHandle<()> {
    supervised_spawn("mm_sla_evaluator", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            let mut program = exchange
                .market_maker_program
                .lock().unwrap_or_else(|e| e.into_inner());

            // Collect the set of active MM user IDs to avoid borrow conflicts.
            let active_mm_ids: Vec<Uuid> = program
                .records
                .iter()
                .filter(|(_, r)| r.is_active)
                .map(|(id, _)| *id)
                .collect();

            let symbols: Vec<_> = program.monitors.keys().cloned().collect();

            for mm_id in &active_mm_ids {
                for symbol in &symbols {
                    // Evaluate; errors mean the MM is not registered in this
                    // monitor (e.g. registered after the monitor was created).
                    let report = match program
                        .monitors
                        .get_mut(symbol)
                        .and_then(|m| m.demote_if_non_compliant(*mm_id, now).ok())
                    {
                        Some(r) => r,
                        None => continue,
                    };

                    // Update aggregate record.
                    if let Some(rec) = program.records.get_mut(mm_id) {
                        rec.last_evaluated_at = Some(now);

                        if let Some(action) = report.demotion {
                            use exchange_market_maker::DemotionAction;
                            rec.total_violations += 1;
                            match action {
                                DemotionAction::Warning => {
                                    if rec.penalty_level < 1 {
                                        rec.penalty_level = 1;
                                    }
                                    tracing::warn!(
                                        mm_id = %mm_id,
                                        symbol = %symbol,
                                        "MM SLA warning issued"
                                    );
                                }
                                DemotionAction::ReduceRebates => {
                                    if rec.penalty_level < 2 {
                                        rec.penalty_level = 2;
                                    }
                                    tracing::warn!(
                                        mm_id = %mm_id,
                                        symbol = %symbol,
                                        "MM SLA: rebates reduced due to repeated violations"
                                    );
                                }
                                DemotionAction::SuspendMm => {
                                    rec.penalty_level = 3;
                                    rec.is_active = false;
                                    tracing::error!(
                                        mm_id = %mm_id,
                                        symbol = %symbol,
                                        "MM suspended due to sustained SLA breaches"
                                    );
                                }
                            }
                        }
                    }

                    // Store report (cap at 10 000 entries).
                    program.reports.push(report);
                    if program.reports.len() > 10_000 {
                        program.reports.remove(0);
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("mm_sla_evaluator");
        }
        })
    })
}

/// Background task: update rolling performance stats every 10 seconds.
///
/// Refreshes live gauges (active orders, open positions, orderbook depth) and
/// logs a summary of throughput and latency so operators can monitor the
/// exchange health without polling the HTTP endpoints.
pub fn spawn_performance_stats_updater(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("performance_stats_updater", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(10));
        loop {
            ticker.tick().await;

            // --- Active orders gauge ---
            let active_orders = exchange.order_locks.len() as u64;
            exchange.metrics.set_active_orders(active_orders);

            // --- Open positions gauge ---
            let open_positions = {
                let pm = exchange.positions.lock();
                pm.all_positions().len() as u64
            };
            exchange.metrics.set_open_positions(open_positions);

            // --- Log throughput summary ---
            let throughput = exchange.metrics.throughput_stats();
            tracing::debug!(
                orders_per_sec_1m = %format!("{:.2}", throughput.orders_per_sec_1m),
                fills_per_sec_1m = %format!("{:.2}", throughput.fills_per_sec_1m),
                active_orders = active_orders,
                open_positions = open_positions,
                "performance_stats_tick"
            );
            exchange.metrics.touch_task_heartbeat("performance_stats_updater");
        }
        })
    })
}

/// Remove expired listen keys every 60 seconds.
///
/// A listen key expires 60 minutes after creation or last keepalive.
/// This task purges entries whose `expires_at` has passed so the map does not
/// grow without bound.
pub fn spawn_listen_key_expiry(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("listen_key_expiry", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = chrono::Utc::now();
            let mut keys = exchange.listen_keys.lock().unwrap_or_else(|e| e.into_inner());
            let before = keys.len();
            keys.retain(|_, info| info.expires_at > now);
            let removed = before - keys.len();
            if removed > 0 {
                tracing::debug!(removed, "expired listen keys purged");
            }
            exchange.metrics.touch_task_heartbeat("listen_key_expiry");
        }
        })
    })
}

/// Accrue interest on all active spot margin borrows every hour.
///
/// For each active `MarginBorrow` the hourly interest (`daily_rate / 24 * amount`)
/// is credited to `interest_accrued`.  A `MarginInterestRecord` is appended for
/// the history endpoint.  The record list is capped at 500 000 entries.
///
/// Auto-liquidation: if the accrued interest + principal exceeds 1.1×
/// the user's available balance in the borrowed coin, the full available
/// balance is used to repay (partial force-repay) and a warning is logged.
pub fn spawn_margin_interest_accrual(
    exchange: Arc<Exchange>,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    supervised_spawn_with_shutdown("margin_interest_accrual", move || {
        let exchange = Arc::clone(&exchange);
        let mut shutdown_rx = shutdown_rx.clone();
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(3600)); // every hour
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("margin_interest_accrual: shutdown signal received — exiting");
                        return;
                    }
                }
            }
            let now = chrono::Utc::now();

            // Snapshot active borrows (user_id, coin) pairs.
            let borrow_keys: Vec<(uuid::Uuid, String)> = {
                let borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
                borrows
                    .iter()
                    .filter(|(_, b)| b.status == "active")
                    .map(|(k, _)| k.clone())
                    .collect()
            };

            for key in borrow_keys {
                // Compute hourly interest on this borrow.
                let (borrow_id, hourly_interest, coin, user_id) = {
                    let mut borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
                    let borrow = match borrows.get_mut(&key) {
                        Some(b) if b.status == "active" => b,
                        _ => continue,
                    };
                    let hourly_rate = borrow.daily_rate / Decimal::from(24u32);
                    let interest = borrow.amount * hourly_rate;
                    borrow.interest_accrued += interest;
                    (borrow.id, interest, borrow.coin.clone(), borrow.user_id)
                };

                if hourly_interest.is_zero() {
                    continue;
                }

                // Record the charge.
                {
                    let mut history = exchange
                        .margin_interest_history
                        .lock().unwrap_or_else(|e| e.into_inner());
                    history.push(crate::exchange::MarginInterestRecord {
                        id: uuid::Uuid::new_v4(),
                        borrow_id,
                        user_id,
                        coin: coin.clone(),
                        interest_amount: hourly_interest,
                        charged_at: now,
                    });
                    const MAX_INTEREST_HISTORY: usize = 500_000;
                    let current_len = history.len();
                    if current_len > MAX_INTEREST_HISTORY {
                        history.drain(0..current_len - MAX_INTEREST_HISTORY);
                    }
                }

                tracing::debug!(
                    user_id = %user_id,
                    coin = %coin,
                    interest = %hourly_interest,
                    "margin_interest_accrued"
                );

                // Collected margin interest feeds the yield reserve pool,
                // which in turn backs earn and staking payouts. This creates a
                // sustainable loop: borrower interest → yield pool → earn/staking.
                {
                    use crate::exchange::YIELD_POOL_ACCOUNT_ID;
                    let _ = exchange.accounts.deposit(YIELD_POOL_ACCOUNT_ID, &coin, hourly_interest);
                }

                // Auto-liquidation check: if debt/collateral ratio > 1.1
                // force-repay using available balance.
                let (total_owed, available_balance) = {
                    let borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
                    let owed = borrows
                        .get(&key)
                        .map(|b| b.amount + b.interest_accrued)
                        .unwrap_or(Decimal::ZERO);
                    let avail = exchange
                        .accounts
                        .get_balance(user_id, &coin)
                        .map(|b| b.available())
                        .unwrap_or(Decimal::ZERO);
                    (owed, avail)
                };

                // Liquidation threshold: debt > collateral * 1.1.
                // (Rearranged: debt / collateral > 1.1)
                let liquidation_threshold = Decimal::new(11, 1); // 1.1
                if available_balance > Decimal::ZERO
                    && total_owed > available_balance * liquidation_threshold
                {
                    tracing::warn!(
                        user_id = %user_id,
                        coin = %coin,
                        total_owed = %total_owed,
                        available = %available_balance,
                        "spot_margin_auto_liquidation: collateral ratio below 1.1, force-repaying"
                    );

                    // Force-repay up to full available balance.
                    let repay = available_balance.min(total_owed);
                    let _ = exchange.accounts.withdraw(user_id, &coin, repay);
                    {
                        let mut borrows = exchange
                            .margin_borrows
                            .lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(borrow) = borrows.get_mut(&key) {
                            // Apply against interest first, then principal.
                            let interest_paid = repay.min(borrow.interest_accrued);
                            let principal_paid = repay - interest_paid;
                            borrow.interest_accrued -= interest_paid;
                            borrow.amount = (borrow.amount - principal_paid).max(Decimal::ZERO);
                            if (borrow.amount + borrow.interest_accrued).is_zero() {
                                borrow.status = "repaid".into();
                            }
                        }
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("margin_interest_accrual");
        }
        })
    })
}

/// Accrue yield on all active earn allocations every hour.
///
/// For each active allocation:
/// - `hourly_rate = apy / 8760`
/// - `interest = amount * hourly_rate`
///
/// When `auto_compound` is true the interest is added to `amount` (the
/// principal grows); otherwise it accumulates in `earned_interest`.
///
/// The `last_accrued_at` timestamp is updated each cycle so the redeem
/// handler can correctly compute partial-hour interest at withdrawal time.
async fn run_earn_yield_accrual(exchange: Arc<Exchange>) {
    let mut ticker = interval(Duration::from_secs(3600)); // every hour
    loop {
        ticker.tick().await;

            // Collect product APYs once per tick to avoid repeated lock acquisitions.
            let product_apys: std::collections::HashMap<uuid::Uuid, Decimal> = {
                let products = exchange.earn_products.lock().unwrap_or_else(|e| e.into_inner());
                products.iter().map(|p| (p.id, p.apy)).collect()
            };

            let mut total_credited = 0usize;
            {
                let mut allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
                let now = chrono::Utc::now();

                for alloc in allocs.iter_mut() {
                    if alloc.status != "active" {
                        continue;
                    }
                    // For fixed products also check the allocation hasn't expired.
                    // If it has, keep it active but do not force-redeem here —
                    // the user must call the redeem endpoint.
                    let apy = match product_apys.get(&alloc.product_id) {
                        Some(&a) => a,
                        None => continue,
                    };
                    if apy.is_zero() {
                        continue;
                    }

                    let hourly_rate = apy / Decimal::from(8760u32);
                    let mut interest = alloc.amount * hourly_rate;

                    // Cap interest to yield pool balance — prevents creating
                    // liability beyond what the pool can cover.
                    {
                        use crate::exchange::YIELD_POOL_ACCOUNT_ID;
                        let pool_balance = exchange
                            .accounts
                            .get_balance(YIELD_POOL_ACCOUNT_ID, &alloc.coin)
                            .map(|b| b.available())
                            .unwrap_or(Decimal::ZERO);
                        if pool_balance < interest {
                            tracing::warn!(
                                user_id = %alloc.user_id,
                                coin = %alloc.coin,
                                pool_balance = %pool_balance,
                                interest_owed = %interest,
                                "earn_yield_accrual: yield pool insufficient — capping interest accrual"
                            );
                            interest = pool_balance;
                        }
                    }

                    if interest.is_zero() {
                        continue;
                    }

                    // Debit yield pool — double-entry: every credit has a corresponding debit.
                    {
                        use crate::exchange::YIELD_POOL_ACCOUNT_ID;
                        if let Err(e) = exchange.accounts.withdraw(YIELD_POOL_ACCOUNT_ID, &alloc.coin, interest) {
                            tracing::error!(
                                coin = %alloc.coin,
                                interest = %interest,
                                error = %e,
                                "earn_yield_accrual: yield pool debit failed — skipping interest"
                            );
                            continue;
                        }
                    }

                    if alloc.auto_compound {
                        alloc.amount += interest;
                    } else {
                        alloc.earned_interest += interest;
                    }
                    alloc.last_accrued_at = now;
                    total_credited += 1;
                }
            }

            if total_credited > 0 {
                tracing::debug!(
                    allocations = total_credited,
                    "earn_yield_accrual: interest credited"
                );
            }
    }
}

/// Execute due recurring buy plans every minute.
///
/// For each active plan whose `next_execution` timestamp is in the past, the
/// task places a market buy order for `amount_usdt` of `coin` (e.g. buying
/// BTC with USDT on the BTCUSDT pair).  After a successful order the plan's
/// `next_execution` is advanced to the next schedule slot.
pub fn spawn_recurring_buy_executor(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("recurring_buy_executor", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Snapshot plan IDs and metadata that are due.
            #[allow(clippy::type_complexity)]
            let due_plans: Vec<(Uuid, Uuid, String, Decimal, crate::exchange::RecurringBuyFrequency, Option<u8>, u8)> = {
                let plans = exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner());
                plans
                    .iter()
                    .filter(|p| p.is_active && p.next_execution <= now)
                    .map(|p| (p.id, p.user_id, p.coin.clone(), p.amount_usdt, p.frequency, p.day_of_week, p.hour))
                    .collect()
            };

            for (plan_id, user_id, coin, amount_usdt, frequency, day_of_week, hour) in due_plans {
                // Build the trading pair symbol (e.g. BTCUSDT).
                let symbol = format!("{coin}USDT");

                // Derive quantity from the current mid-price so we can place a
                // market order.  If no price is available we skip this cycle.
                let mid_price = exchange
                    .market_data
                    .ticker(&exchange_common::Symbol::new(symbol.as_str()))
                    .and_then(|t| {
                        if t.last_price > Decimal::ZERO {
                            Some(t.last_price)
                        } else {
                            None
                        }
                    });

                let Some(price) = mid_price else {
                    tracing::warn!(
                        plan_id = %plan_id,
                        symbol = %symbol,
                        "recurring_buy: no price available — skipping this cycle"
                    );
                    continue;
                };

                let qty = amount_usdt / price;
                if qty <= Decimal::ZERO {
                    continue;
                }

                let cmd = PlaceOrderCommand {
                    order_id: Uuid::new_v4(),
                    user_id,
                    symbol: exchange_common::Symbol::new(symbol.as_str()),
                    side: exchange_common::Side::Buy,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Ioc,
                    price: Decimal::ZERO,
                    qty,
                    reduce_only: false,
                    expire_at: None,
                    timestamp: now,
                    position_idx: 0,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: StpMode::default(),
                    request_expire_time: None,
                };

                match exchange.place_order(cmd) {
                    Ok(_) => {
                        tracing::info!(
                            plan_id = %plan_id,
                            user_id = %user_id,
                            symbol = %symbol,
                            amount_usdt = %amount_usdt,
                            qty = %qty,
                            "recurring_buy: order placed"
                        );

                        // Advance schedule.
                        let mut plans = exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(plan) = plans.iter_mut().find(|p| p.id == plan_id) {
                            plan.next_execution = crate::rest_algo::compute_next_execution(
                                frequency,
                                day_of_week,
                                hour,
                                now,
                            );
                            plan.total_invested += amount_usdt;
                            plan.executions += 1;
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            plan_id = %plan_id,
                            symbol = %symbol,
                            error = %e,
                            "recurring_buy: order failed"
                        );
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("recurring_buy_executor");
        }
        })
    })
}

/// Snapshot open interest and long/short ratios for every configured perpetual symbol
/// every 5 minutes.
///
/// Each snapshot captures:
/// - Total open interest (base asset) and its USD notional at the current mark price.
/// - Aggregate long vs short position sizes and the resulting ratio.
///
/// Results are appended to `exchange.oi_history` and `exchange.long_short_history`.
/// Both maps are capped at [`MAX_STATS_HISTORY_PER_SYMBOL`] entries per symbol.
pub fn spawn_oi_snapshotter(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("oi_snapshotter", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(300)); // every 5 minutes
        loop {
            ticker.tick().await;
            let now = Utc::now();
            let ts = now.timestamp();

            // Collect the configured symbols once (no lock held — ArcSwap load).
            let symbols: Vec<_> = exchange
                .perp_configs
                .load()
                .keys()
                .cloned()
                .collect();

            for symbol in &symbols {
                let (long_size, short_size) = {
                    let pm = exchange.positions.lock();
                    let mut long = rust_decimal::Decimal::ZERO;
                    let mut short = rust_decimal::Decimal::ZERO;
                    for pos in pm.all_positions() {
                        if &pos.symbol == symbol {
                            match pos.side {
                                exchange_common::futures::PositionSide::Long => long += pos.size,
                                exchange_common::futures::PositionSide::Short => short += pos.size,
                            }
                        }
                    }
                    (long, short)
                };

                let total = long_size + short_size;
                let mark = exchange.get_mark_price(symbol).map(|mp| mp.price);
                let oi_usd = mark
                    .map(|m| total * m)
                    .unwrap_or(rust_decimal::Decimal::ZERO);

                // --- OI snapshot ---
                {
                    let mut hist = exchange.oi_history.lock().unwrap_or_else(|e| e.into_inner());
                    let entries = hist.entry(symbol.clone()).or_default();
                    entries.push_back(crate::exchange::OiSnapshot {
                        timestamp: ts,
                        open_interest: total,
                        open_interest_value_usd: oi_usd,
                    });
                    if entries.len() > crate::exchange::MAX_STATS_HISTORY_PER_SYMBOL {
                        entries.pop_front();
                    }
                }

                // --- Long/Short snapshot ---
                let (long_ratio, short_ratio) = if total > rust_decimal::Decimal::ZERO {
                    (long_size / total, short_size / total)
                } else {
                    (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
                };
                {
                    let mut hist = exchange.long_short_history.lock().unwrap_or_else(|e| e.into_inner());
                    let entries = hist.entry(symbol.clone()).or_default();
                    entries.push_back(crate::exchange::LongShortSnapshot {
                        timestamp: ts,
                        long_ratio,
                        short_ratio,
                        long_volume: long_size,
                        short_volume: short_size,
                    });
                    if entries.len() > crate::exchange::MAX_STATS_HISTORY_PER_SYMBOL {
                        entries.pop_front();
                    }
                }

                tracing::debug!(
                    symbol = %symbol,
                    oi = %total,
                    oi_usd = %oi_usd,
                    long_ratio = %long_ratio,
                    short_ratio = %short_ratio,
                    "oi_snapshotter: snapshot recorded"
                );
            }
            exchange.metrics.touch_task_heartbeat("oi_snapshotter");
        }
        })
    })
}

// =============================================================================
// Advanced Financial Products — background tasks
// =============================================================================

/// Accrue staking rewards every hour for all active ETH/SOL positions.
///
/// ETH: ~4 % APY → hourly_rate = 0.04 / 8760
/// SOL: ~7 % APY → hourly_rate = 0.07 / 8760
///
/// The accrued rewards accumulate in `StakingPosition::pending_rewards`.
/// They are **not** automatically credited to the user's balance; the user
/// must claim them (or they are included in the position balance query).
pub fn spawn_staking_rewards_accrual(exchange: Arc<Exchange>) -> JoinHandle<()> {
    use crate::exchange::{StakingApySnapshot, STAKING_APY, MAX_STAKING_APY_HISTORY};

    supervised_spawn("staking_rewards_accrual", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(3600)); // every hour
        loop {
            ticker.tick().await;
            let now = Utc::now();

            let apy_map: std::collections::HashMap<String, Decimal> = STAKING_APY
                .iter()
                .filter_map(|(coin, apy_str)| {
                    apy_str.parse::<Decimal>().ok().map(|a| (coin.to_string(), a))
                })
                .collect();

            let mut rewarded = 0usize;
            {
                let mut positions = exchange
                    .staking_positions
                    .lock().unwrap_or_else(|e| e.into_inner());

                for ((_, coin), pos) in positions.iter_mut() {
                    if pos.staked_amount <= Decimal::ZERO {
                        continue;
                    }
                    let apy = match apy_map.get(coin) {
                        Some(&a) => a,
                        None => continue,
                    };
                    let hourly_rate = apy / Decimal::from(8760u32);
                    let mut reward = pos.staked_amount * hourly_rate;

                    // Cap reward to yield pool balance.
                    {
                        use crate::exchange::YIELD_POOL_ACCOUNT_ID;
                        let pool_balance = exchange
                            .accounts
                            .get_balance(YIELD_POOL_ACCOUNT_ID, coin)
                            .map(|b| b.available())
                            .unwrap_or(Decimal::ZERO);
                        if pool_balance < reward {
                            tracing::warn!(
                                user_id = %pos.user_id,
                                coin = %coin,
                                pool_balance = %pool_balance,
                                reward_owed = %reward,
                                "staking_rewards_accrual: yield pool insufficient — capping reward"
                            );
                            reward = pool_balance;
                        }
                    }

                    if reward.is_zero() {
                        continue;
                    }

                    pos.pending_rewards += reward;
                    pos.last_accrued_at = now;
                    rewarded += 1;
                }
            }

            // Record APY snapshots.
            {
                let mut hist = exchange
                    .staking_apy_history
                    .lock().unwrap_or_else(|e| e.into_inner());

                for (coin, apy) in &apy_map {
                    let entries = hist.entry(coin.clone()).or_default();
                    entries.push(StakingApySnapshot {
                        coin: coin.clone(),
                        apy: *apy,
                        recorded_at: now,
                    });
                    if entries.len() > MAX_STAKING_APY_HISTORY {
                        entries.remove(0);
                    }
                }
            }

            if rewarded > 0 {
                tracing::debug!(
                    positions = rewarded,
                    "staking_rewards_accrual: rewards credited"
                );
            }
            exchange.metrics.touch_task_heartbeat("staking_rewards_accrual");
        }
        })
    })
}

/// Check the unbonding queue every minute and release matured entries.
///
/// When `available_at` ≤ now, the staked coins are credited back to the
/// user's trading balance and the entry is removed from the queue.
pub fn spawn_staking_unbonding_release(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("staking_unbonding_release", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Collect matured entries — do not hold the lock across the account mutation.
            let matured: Vec<crate::exchange::UnbondingEntry> = {
                let mut unbonding = exchange
                    .staking_unbonding
                    .lock().unwrap_or_else(|e| e.into_inner());

                let (ready, pending): (Vec<_>, Vec<_>) =
                    unbonding.drain(..).partition(|e| e.available_at <= now);
                *unbonding = pending;
                ready
            };

            for entry in matured {
                let _ = exchange.accounts.deposit(entry.user_id, &entry.coin, entry.amount);
                tracing::info!(
                    user_id = %entry.user_id,
                    coin = %entry.coin,
                    amount = %entry.amount,
                    unbonding_id = %entry.id,
                    "staking_unbonding_release: coins returned to user",
                );
            }
            exchange.metrics.touch_task_heartbeat("staking_unbonding_release");
        }
        })
    })
}

/// Settle expired dual investment positions every minute.
///
/// For each unsettled position whose product has passed its expiry time:
/// - Fetch the current mark price for the underlying coin.
/// - `BuyLow` product: if mark ≤ strike → convert USDT to coin at strike price;
///   otherwise return USDT principal + premium.
/// - `SellHigh` product: if mark ≥ strike → user sold coin at strike (receives USDT);
///   otherwise return coin principal + premium.
/// - Premium = amount × APY / 365 × days_to_expiry (prorated).
pub fn spawn_dual_invest_settler(exchange: Arc<Exchange>) -> JoinHandle<()> {
    use crate::exchange::{DualInvestDirection, DualInvestStatus};

    supervised_spawn("dual_invest_settler", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Snapshot expired product IDs and their metadata.
            let expired_products: Vec<(uuid::Uuid, String, DualInvestDirection, Decimal, Decimal, chrono::DateTime<Utc>)> = {
                let mut products = exchange
                    .dual_invest_products
                    .lock().unwrap_or_else(|e| e.into_inner());

                let mut result = Vec::new();
                for p in products.iter_mut() {
                    if p.status == DualInvestStatus::Active && p.expiry <= now {
                        p.status = DualInvestStatus::Closed;
                        result.push((p.id, p.underlying.clone(), p.direction, p.strike_price, p.apy, p.expiry));
                    }
                }
                result
            };

            if expired_products.is_empty() {
                continue;
            }

            // Get mark prices once.
            let mark_prices = exchange.get_mark_prices();

            for (product_id, underlying, direction, strike_price, apy, expiry) in expired_products {
                let symbol_key = exchange_common::Symbol::new(format!("{underlying}USDT"));
                let mark_price = match mark_prices.get(&symbol_key).map(|mp| mp.price) {
                    Some(p) if p > Decimal::ZERO => p,
                    _ => {
                        tracing::warn!(
                            product_id = %product_id,
                            underlying = %underlying,
                            "dual_invest_settler: no mark price available, skipping settlement"
                        );
                        continue;
                    }
                };

                // Collect positions for this product.
                let position_ids: Vec<uuid::Uuid> = {
                    let positions = exchange
                        .dual_invest_positions
                        .lock().unwrap_or_else(|e| e.into_inner());
                    positions
                        .iter()
                        .filter(|p| p.product_id == product_id && !p.settled)
                        .map(|p| p.id)
                        .collect()
                };

                for pos_id in position_ids {
                    let (user_id, amount) = {
                        let positions = exchange
                            .dual_invest_positions
                            .lock().unwrap_or_else(|e| e.into_inner());
                        match positions.iter().find(|p| p.id == pos_id) {
                            Some(p) => (p.user_id, p.amount),
                            None => continue,
                        }
                    };

                    // Calculate tenor in days for the premium.
                    let days_elapsed = {
                        let positions = exchange
                            .dual_invest_positions
                            .lock().unwrap_or_else(|e| e.into_inner());
                        match positions.iter().find(|p| p.id == pos_id) {
                            Some(p) => {
                                let d = (expiry - p.subscribed_at).num_seconds();
                                Decimal::from(d.max(0) as u64) / Decimal::from(86400u32)
                            }
                            None => continue,
                        }
                    };

                    let (payout, payout_coin) = match direction {
                        DualInvestDirection::BuyLow => {
                            // amount is USDT.
                            let premium_usdt = amount * apy / Decimal::from(365u32) * days_elapsed;
                            if mark_price <= strike_price {
                                // Strike hit: convert USDT → coin at strike price.
                                let coin_amount = (amount + premium_usdt) / strike_price;
                                (coin_amount, underlying.clone())
                            } else {
                                // Strike not hit: return USDT + premium.
                                (amount + premium_usdt, "USDT".to_string())
                            }
                        }
                        DualInvestDirection::SellHigh => {
                            // amount is the underlying coin.
                            let premium_coin = amount * apy / Decimal::from(365u32) * days_elapsed;
                            if mark_price >= strike_price {
                                // Strike hit: sell coin at strike → receive USDT.
                                let usdt_amount = (amount + premium_coin) * strike_price;
                                (usdt_amount, "USDT".to_string())
                            } else {
                                // Strike not hit: return coin + premium.
                                (amount + premium_coin, underlying.clone())
                            }
                        }
                    };

                    // Credit payout to user.
                    let _ = exchange.accounts.deposit(user_id, &payout_coin, payout);

                    // Mark position settled.
                    {
                        let mut positions = exchange
                            .dual_invest_positions
                            .lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(p) = positions.iter_mut().find(|p| p.id == pos_id) {
                            p.settled = true;
                            p.payout = Some(payout);
                            p.payout_coin = Some(payout_coin.clone());
                        }
                    }

                    tracing::info!(
                        user_id = %user_id,
                        position_id = %pos_id,
                        product_id = %product_id,
                        mark_price = %mark_price,
                        strike_price = %strike_price,
                        payout = %payout,
                        payout_coin = %payout_coin,
                        "dual_invest_settler: position settled",
                    );
                }

                // Mark product as fully settled.
                {
                    let mut products = exchange
                        .dual_invest_products
                        .lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(p) = products.iter_mut().find(|p| p.id == product_id) {
                        p.status = DualInvestStatus::Settled;
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("dual_invest_settler");
        }
        })
    })
}

/// Accrue hourly interest on all active crypto loans and auto-liquidate
/// positions whose LTV has exceeded the 85 % threshold.
///
/// Interest = loan_amount × daily_rate / 24 (hourly).
///
/// LTV is recalculated using the current mark price on each tick.
/// When LTV > 85 %: collateral is seized (removed), loan is marked `Liquidated`,
/// and the treasury receives the collateral value.
async fn run_loan_interest_accrual(exchange: Arc<Exchange>) {
    use crate::exchange::{LoanStatus, default_loan_collateral_info};

    let mut ticker = interval(Duration::from_secs(3600)); // every hour
    loop {
        ticker.tick().await;

            let collateral_configs = default_loan_collateral_info();
            let liquidation_ltv = collateral_configs
                .first()
                .map(|c| c.liquidation_ltv)
                .unwrap_or(Decimal::new(85, 2));

            let mark_prices = exchange.get_mark_prices();

            let mut total_interest = 0usize;
            let mut total_liquidated = 0usize;

            // Snapshot loan IDs to iterate without holding the lock.
            let loan_ids: Vec<uuid::Uuid> = {
                let loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
                loans
                    .iter()
                    .filter(|l| l.status == LoanStatus::Active)
                    .map(|l| l.id)
                    .collect()
            };

            for loan_id in loan_ids {
                let (user_id, loan_amount, interest_rate, collateral_coin, collateral_amount, loan_coin) = {
                    let loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
                    match loans.iter().find(|l| l.id == loan_id) {
                        Some(l) => (
                            l.user_id,
                            l.loan_amount,
                            l.interest_rate,
                            l.collateral_coin.clone(),
                            l.collateral_amount,
                            l.loan_coin.clone(),
                        ),
                        None => continue,
                    }
                };

                // Accrue hourly interest.
                let hourly_interest = loan_amount * interest_rate / Decimal::from(24u32);
                let new_ltv = {
                    let symbol_key = exchange_common::Symbol::new(format!("{collateral_coin}USDT"));
                    let collateral_price = mark_prices
                        .get(&symbol_key)
                        .map(|mp| mp.price)
                        .unwrap_or(Decimal::ZERO);

                    let mut loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(l) = loans.iter_mut().find(|l| l.id == loan_id) {
                        l.interest_accrued += hourly_interest;
                        total_interest += 1;

                        if collateral_price > Decimal::ZERO && loan_coin == "USDT" {
                            let collateral_value = collateral_amount * collateral_price;
                            l.ltv = (l.loan_amount + l.interest_accrued) / collateral_value;
                        }
                        l.ltv
                    } else {
                        continue;
                    }
                };

                // Auto-liquidate if LTV exceeds threshold.
                if new_ltv > liquidation_ltv {
                    {
                        let mut loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(l) = loans.iter_mut().find(|l| l.id == loan_id) {
                            l.status = LoanStatus::Liquidated;
                            l.closed_at = Some(Utc::now());
                        }
                    }

                    // Transfer collateral to treasury.
                    use crate::exchange::TREASURY_ACCOUNT_ID;
                    let _ = exchange.accounts.deposit(TREASURY_ACCOUNT_ID, &collateral_coin, collateral_amount);

                    total_liquidated += 1;
                    tracing::warn!(
                        loan_id = %loan_id,
                        user_id = %user_id,
                        ltv = %new_ltv,
                        liquidation_ltv = %liquidation_ltv,
                        collateral_coin = %collateral_coin,
                        collateral_amount = %collateral_amount,
                        "loan_interest_accrual: LTV exceeded threshold, loan liquidated",
                    );
                }
            }

            if total_interest > 0 || total_liquidated > 0 {
                tracing::debug!(
                    accrued = total_interest,
                    liquidated = total_liquidated,
                    "loan_interest_accrual: tick complete"
                );
            }
    }
}

/// Settle expired Shark Fin products every 60 seconds.
///
/// For each expired product with active positions:
/// - Check if spot price hit the knock-in or knock-out barrier
/// - Apply `guaranteed_apy` if barrier was breached, `max_apy` if price stayed in range
/// - Credit principal + yield to user, mark position as settled
pub fn spawn_shark_fin_settler(exchange: Arc<Exchange>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Find expired products
            let expired_ids: Vec<uuid::Uuid> = {
                let products = exchange.shark_fin_products.lock().unwrap_or_else(|e| e.into_inner());
                products.iter()
                    .filter(|p| p.is_active && p.settle_at <= now)
                    .map(|p| p.id)
                    .collect()
            };

            for prod_id in expired_ids {
                // Get product details
                let product = {
                    let products = exchange.shark_fin_products.lock().unwrap_or_else(|e| e.into_inner());
                    products.iter().find(|p| p.id == prod_id).cloned()
                };
                let Some(product) = product else { continue };

                // Get mark price for the underlying (assume BTCUSDT)
                let mark_price = exchange.get_mark_price(&exchange_common::Symbol::new("BTCUSDT"))
                    .map(|mp| mp.price)
                    .unwrap_or(Decimal::ZERO);
                if mark_price.is_zero() { continue; }

                // Determine which APY applies
                let breached = if product.direction == "bullish" {
                    mark_price >= product.knock_out
                } else {
                    mark_price <= product.knock_in
                };
                let apy = if breached { product.guaranteed_apy } else { product.max_apy };

                // Settle active positions for this product
                let mut positions = exchange.shark_fin_positions.lock().unwrap_or_else(|e| e.into_inner());
                for pos in positions.iter_mut() {
                    if pos.product_id != prod_id || pos.status != crate::rest_products::SharkFinStatus::Active {
                        continue;
                    }

                    // Calculate yield: amount × APY × (duration_days / 365)
                    let duration_days = (product.settle_at - pos.subscribed_at).num_days().max(1);
                    let yield_amount = pos.amount * apy * Decimal::from(duration_days) / Decimal::from(365);
                    let total = pos.amount + yield_amount;

                    // Credit user
                    let _ = exchange.accounts.deposit(pos.user_id, "USDT", total);

                    pos.status = crate::rest_products::SharkFinStatus::Settled;
                    pos.yield_paid = Some(yield_amount);
                    pos.settled_at = Some(now);

                    tracing::info!(
                        user_id = %pos.user_id,
                        product_id = %prod_id,
                        yield_amount = %yield_amount,
                        breached = breached,
                        "shark_fin: position settled"
                    );
                }

                // Mark product as inactive
                let mut products = exchange.shark_fin_products.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(p) = products.iter_mut().find(|p| p.id == prod_id) {
                    p.is_active = false;
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Engine latency metrics updater (KG-3 / KG-4 / KG-5)
// ---------------------------------------------------------------------------

/// Periodically polls engine latency snapshots and updates aggregate Prometheus gauges.
///
/// Runs two independent timers:
/// - Every 30 seconds: collect latency snapshots from all engine threads and
///   update the 6 aggregate gauges on `ExchangeMetrics` (max p50/p99/p99.9/
///   p99.99, max-of-max, total count).
/// - Every 5 minutes: send `ResetLatencyHistogram` to all engines so that the
///   gauges represent a rolling 5-minute window rather than lifetime worst-case.
///
/// KG-5 contract: only aggregate (max-of-symbols) metrics are pushed to
/// `ExchangeMetrics`. Per-symbol latency is NOT exposed — this prevents HFT
/// asymmetry leak via matching engine congestion signals. The per-symbol data
/// stays inside the engine threads and is never surfaced on the public
/// `/metrics` endpoint. Internal debugging can use the authenticated
/// `/admin/engine/latency` endpoint (future sprint).
///
/// Errors from individual engine threads (e.g. disconnected) are logged at
/// WARN level and skipped — a single unresponsive engine does not block the
/// update for all other symbols.
pub fn spawn_latency_metrics_updater(exchange: Arc<Exchange>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut refresh_interval = tokio::time::interval(Duration::from_secs(30));
        let mut reset_interval = tokio::time::interval(Duration::from_secs(5 * 60));
        loop {
            tokio::select! {
                _ = refresh_interval.tick() => {
                    update_aggregate_latency_gauges(&exchange);
                    exchange.metrics.touch_task_heartbeat("latency_metrics_updater");
                }
                _ = reset_interval.tick() => {
                    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                    let results = pool.reset_all_latency_histograms();
                    let errors: usize = results.iter().filter(|r| r.is_err()).count();
                    if errors > 0 {
                        tracing::warn!(errors, "latency_metrics_updater: {} engine(s) failed histogram reset", errors);
                    } else {
                        tracing::debug!("latency_metrics_updater: histogram reset sent to all engines");
                    }
                }
            }
        }
    })
}

fn update_aggregate_latency_gauges(exchange: &Exchange) {
    use std::sync::atomic::Ordering;

    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
    let symbols = pool.symbols();

    let mut max_p50: u64 = 0;
    let mut max_p99: u64 = 0;
    let mut max_p999: u64 = 0;
    let mut max_p9999: u64 = 0;
    let mut max_overall: u64 = 0;
    let mut total_count: u64 = 0;

    for symbol in &symbols {
        match pool.latency_snapshot(symbol) {
            Ok(snap) => {
                max_p50 = max_p50.max(snap.p50_ns);
                max_p99 = max_p99.max(snap.p99_ns);
                max_p999 = max_p999.max(snap.p999_ns);
                max_p9999 = max_p9999.max(snap.p9999_ns);
                max_overall = max_overall.max(snap.max_ns);
                total_count = total_count.saturating_add(snap.count);
            }
            Err(e) => {
                tracing::warn!(symbol = %symbol, error = %e, "latency_metrics_updater: snapshot failed");
            }
        }
    }

    exchange.metrics.engine_latency_p50_max_ns.store(max_p50, Ordering::Relaxed);
    exchange.metrics.engine_latency_p99_max_ns.store(max_p99, Ordering::Relaxed);
    exchange.metrics.engine_latency_p999_max_ns.store(max_p999, Ordering::Relaxed);
    exchange.metrics.engine_latency_p9999_max_ns.store(max_p9999, Ordering::Relaxed);
    exchange.metrics.engine_latency_max_overall_ns.store(max_overall, Ordering::Relaxed);
    exchange.metrics.engine_total_command_count.store(total_count, Ordering::Relaxed);

    tracing::debug!(
        max_p50,
        max_p99,
        max_p999,
        max_p9999,
        max_overall,
        total_count,
        symbols = symbols.len(),
        "latency_metrics_updater: aggregate gauges updated"
    );
}

/// Spawn all background tasks.
///
/// `shutdown_rx` is a watch channel that fires `true` when the server is
/// shutting down. Financial-critical tasks (`funding_settler`,
/// `trigger_checker`, `gtd_expiry_checker`, `liquidation_monitor`,
/// `margin_interest_accrual`) select on this channel so they can exit cleanly
/// instead of being hard-killed after the 5-second timeout.
///
/// Returns the join handles so the caller can abort them on shutdown.
/// Spawn all background tasks in the correct dependency order.
///
/// # Startup ordering (documented for FMEA / incident response)
///
/// **Tier 1 — Core (no dependencies, must start first):**
/// - Audit log flusher (durable persistence of security-relevant events)
/// - Performance stats updater (metrics baseline)
/// - Latency metrics updater (engine monitoring)
///
/// **Tier 2 — Risk & settlement (depends on engine pool being ready):**
/// - Liquidation monitor (reads mark prices + positions)
/// - Premium sampler (feeds funding rate calculation)
/// - Funding settler (depends on premium sampler output)
/// - Insurance fund snapshotter
///
/// **Tier 3 — Order lifecycle (depends on matching engine):**
/// - Trigger checker (stop/TP/SL orders — monitors mark prices)
/// - GTD expiry checker (cancels expired orders)
/// - Position snapshotter (WAL snapshots of futures positions)
///
/// **Tier 4 — External integrations (depend on network + config):**
/// - Deposit/withdrawal pollers (custody provider API)
/// - AML/sanctions/surveillance monitors
/// - KYC webhook processing (via Sumsub)
///
/// **Tier 5 — User-facing features (depend on everything above):**
/// - Copy trading settler, bot executor, algo executor
/// - Market maker SLA, leaderboard, price alerts
/// - WS market data fanout (post-fill worker)
///
/// All tasks use `supervised_spawn` for panic restart. Tasks in the same
/// tier are independent and can start concurrently; cross-tier ordering is
/// enforced by the sequential construction of this vec (Tier 1 tasks are
/// first, so they're spawned before Tier 5 tasks, giving them a head start).
pub fn spawn_all_tasks(exchange: Arc<Exchange>, shutdown_rx: watch::Receiver<bool>) -> Vec<JoinHandle<()>> {
    // Assert prerequisites: engine pool must have at least one symbol registered
    // before risk/settlement tasks start querying it.
    {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        let symbol_count = pool.symbols().len();
        if symbol_count == 0 {
            tracing::warn!("No symbols registered in engine pool at task startup — \
                            risk and settlement tasks will skip until products are added");
        } else {
            tracing::info!(symbols = symbol_count, "Engine pool ready — spawning background tasks");
        }
    }

    vec![
        // ── Tier 1: Core infrastructure ──────────────────────────────────
        spawn_audit_log_flusher(Arc::clone(&exchange)),
        spawn_engine_snapshotter(Arc::clone(&exchange)),
        spawn_task_heartbeat_watchdog(Arc::clone(&exchange)),

        // ── Tier 2: Risk & settlement ────────────────────────────────────
        spawn_liquidation_monitor(Arc::clone(&exchange), 1000, shutdown_rx.clone()),
        spawn_premium_sampler(Arc::clone(&exchange)),
        spawn_funding_settler(Arc::clone(&exchange), shutdown_rx.clone()),
        spawn_insurance_fund_snapshotter(Arc::clone(&exchange)),

        // ── Tier 3: Order lifecycle ──────────────────────────────────────
        spawn_trigger_checker(Arc::clone(&exchange), 500, shutdown_rx.clone()),
        spawn_gtd_expiry_checker(Arc::clone(&exchange), shutdown_rx.clone()),
        spawn_position_snapshotter(Arc::clone(&exchange)),

        // ── Tier 4: External integrations ────────────────────────────────
        spawn_reconciliation_job(Arc::clone(&exchange)),
        spawn_ledger_reconciliation(Arc::clone(&exchange)),
        spawn_withdrawal_poller(Arc::clone(&exchange), 10_000),
        spawn_deposit_poller(Arc::clone(&exchange), 10_000),
        spawn_sanctions_rescreener(Arc::clone(&exchange)),
        spawn_ofac_feed_updater(Arc::clone(&exchange)),
        spawn_continuous_aml_monitor(Arc::clone(&exchange)),
        spawn_surveillance_monitor(Arc::clone(&exchange)),
        spawn_withdrawal_confirmation_expiry(Arc::clone(&exchange)),

        // ── Tier 5: User-facing features ─────────────────────────────────
        spawn_dead_mans_switch(Arc::clone(&exchange)),
        spawn_algo_executor(Arc::clone(&exchange)),
        spawn_lp_reward_accrual(Arc::clone(&exchange)),
        // Supervised: restarts on panic.
        supervised_spawn("bot_executor", {
            let ex = Arc::clone(&exchange);
            move || {
                let ex = Arc::clone(&ex);
                Box::pin(async move { run_bot_executor(ex, 5_000).await })
            }
        }),
        spawn_sanctions_rescreener(Arc::clone(&exchange)),
        spawn_ofac_feed_updater(Arc::clone(&exchange)),
        spawn_continuous_aml_monitor(Arc::clone(&exchange)),
        spawn_surveillance_monitor(Arc::clone(&exchange)),
        spawn_price_alert_checker(Arc::clone(&exchange)),
        spawn_withdrawal_confirmation_expiry(Arc::clone(&exchange)),
        spawn_insurance_fund_snapshotter(Arc::clone(&exchange)),
        // Feed MM SLA monitors with order book snapshots every 60 seconds.
        spawn_mm_snapshot_producer(Arc::clone(&exchange)),
        // Evaluate market maker SLA every 5 minutes.
        spawn_mm_sla_evaluator(Arc::clone(&exchange), 300),
        // Update rolling performance stats every 10 seconds.
        spawn_performance_stats_updater(Arc::clone(&exchange)),
        // Purge expired listen keys every minute.
        spawn_listen_key_expiry(Arc::clone(&exchange)),
        // Hourly spot-margin interest accrual — supervised + graceful shutdown.
        spawn_margin_interest_accrual(Arc::clone(&exchange), shutdown_rx.clone()),
        // Hourly earn yield accrual — supervised.
        supervised_spawn("earn_yield_accrual", {
            let ex = Arc::clone(&exchange);
            move || {
                let ex = Arc::clone(&ex);
                Box::pin(async move { run_earn_yield_accrual(ex).await })
            }
        }),
        // Execute scheduled DCA recurring buy plans every minute.
        spawn_recurring_buy_executor(Arc::clone(&exchange)),
        // Snapshot open interest and long/short ratios every 5 minutes.
        spawn_oi_snapshotter(Arc::clone(&exchange)),
        // Hourly staking rewards accrual (ETH ~4 %, SOL ~7 %).
        spawn_staking_rewards_accrual(Arc::clone(&exchange)),
        // Release matured staking unbonding entries every minute.
        spawn_staking_unbonding_release(Arc::clone(&exchange)),
        // Settle expired dual investment positions every minute.
        spawn_dual_invest_settler(Arc::clone(&exchange)),
        spawn_shark_fin_settler(Arc::clone(&exchange)),
        // Cleanup: purge expired convert quotes every 5 minutes
        {
            let ex = Arc::clone(&exchange);
            tokio::spawn(async move {
                let mut t = interval(Duration::from_secs(300));
                loop {
                    t.tick().await;
                    let now = Utc::now();
                    let mut quotes = ex.convert_quotes.lock().unwrap_or_else(|e| e.into_inner());
                    let before = quotes.len();
                    quotes.retain(|_, q| q.expires_at > now);
                    let purged = before - quotes.len();
                    if purged > 0 {
                        tracing::debug!(purged, "convert_quote_cleanup: expired quotes purged");
                    }
                }
            })
        },
        // Cleanup: purge triggered price alerts every hour
        {
            let ex = Arc::clone(&exchange);
            tokio::spawn(async move {
                let mut t = interval(Duration::from_secs(3600));
                loop {
                    t.tick().await;
                    let mut alerts = ex.price_alerts.lock().unwrap_or_else(|e| e.into_inner());
                    let before = alerts.len();
                    alerts.retain(|a| !a.triggered);
                    let purged = before - alerts.len();
                    if purged > 0 {
                        tracing::debug!(purged, "price_alert_cleanup: triggered alerts purged");
                    }
                }
            })
        },
        // Hourly crypto loan interest accrual + auto-liquidation checks — supervised.
        supervised_spawn("loan_interest_accrual", {
            let ex = Arc::clone(&exchange);
            move || {
                let ex = Arc::clone(&ex);
                Box::pin(async move { run_loan_interest_accrual(ex).await })
            }
        }),
        // Broadcast mark prices every 3 seconds on the markPrice.{symbol} WS channel.
        spawn_mark_price_broadcaster(Arc::clone(&exchange)),
        // Broadcast ADL warnings every 30 seconds for top-quantile positions.
        spawn_adl_warning_broadcaster(Arc::clone(&exchange)),
        // Update leveraged token NAV based on underlying mark price every 10 seconds — supervised.
        supervised_spawn("lt_nav_updater", {
            let ex = Arc::clone(&exchange);
            move || {
                let ex = Arc::clone(&ex);
                Box::pin(async move { run_lt_nav_updater(ex).await })
            }
        }),
        // Settle copy trading profit share every hour — supervised.
        supervised_spawn("copy_trading_settlement", {
            let ex = Arc::clone(&exchange);
            move || {
                let ex = Arc::clone(&ex);
                Box::pin(async move { run_copy_trading_settlement(ex).await })
            }
        }),
        // Auto-cancel P2P orders that have been in Created state for > 30 minutes.
        spawn_p2p_timeout_checker(Arc::clone(&exchange)),
        // Decay stale 30-day VIP volumes every hour.
        spawn_vip_volume_decay(Arc::clone(&exchange)),
        // Settle expired option contracts and delivery futures every minute.
        spawn_options_settlement(Arc::clone(&exchange)),
        spawn_delivery_settlement(Arc::clone(&exchange)),
        // Advanced bot executor: infinity-grid, futures-grid, arb, martingale, slicing.
        supervised_spawn("advanced_bot_executor", {
            let ex = Arc::clone(&exchange);
            move || {
                let ex = Arc::clone(&ex);
                Box::pin(async move {
                    crate::rest_bots::run_advanced_bot_executor(ex, 5_000).await
                })
            }
        }),
        // Take a daily account snapshot at midnight UTC; checked every hour.
        spawn_daily_snapshot(Arc::clone(&exchange)),
        // Snapshot premium (mark − index) every 5 minutes for premium-klines.
        spawn_premium_snapshotter(Arc::clone(&exchange)),
        // Broadcast all-market mini-ticker every second on ticker.all WS channel.
        spawn_all_mini_ticker(Arc::clone(&exchange)),
        // Poll engine latency snapshots every 30 s; reset histograms every 5 min (KG-3/KG-4/KG-5).
        spawn_latency_metrics_updater(Arc::clone(&exchange)),
        // S15: Post-fill async worker — surveillance, notifications, WS broadcast, FIX.
        spawn_post_fill_event_worker(Arc::clone(&exchange)),
    ]
}

// ---------------------------------------------------------------------------
// Leveraged token NAV updater
// ---------------------------------------------------------------------------

/// Update NAV for all leveraged tokens every 10 seconds based on the
/// mark price of each token's underlying perpetual.
///
/// NAV formula per tick:
/// ```text
/// nav_new = nav_old * (1 + leverage * (current_price - last_price) / last_price)
/// ```
///
/// On first tick the `last_rebalance_price` is initialised to the current
/// mark price; no NAV change is applied until the second tick.
async fn run_lt_nav_updater(exchange: Arc<Exchange>) {
    let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(10));
    loop {
        ticker.tick().await;

            // Collect the symbols we need mark prices for.
            let tokens_snapshot: Vec<(String, String, i32, rust_decimal::Decimal, Option<rust_decimal::Decimal>)> = {
                let lt = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
                lt.values()
                    .map(|t| (t.symbol.clone(), t.underlying.clone(), t.leverage, t.nav, t.last_rebalance_price))
                    .collect()
            };

            if tokens_snapshot.is_empty() {
                continue;
            }

            let mark_prices = exchange.get_mark_prices();

            let mut lt = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
            for (symbol, underlying, leverage, old_nav, last_price_opt) in tokens_snapshot {
                let underlying_sym = exchange_common::Symbol::new(&underlying);
                let current_price = match mark_prices.get(&underlying_sym) {
                    Some(mp) => mp.price,
                    None => {
                        tracing::debug!(
                            lt_symbol = %symbol,
                            underlying = %underlying,
                            "lt_nav_updater: no mark price for underlying, skipping"
                        );
                        continue;
                    }
                };

                let token = match lt.get_mut(&symbol) {
                    Some(t) => t,
                    None => continue,
                };

                match last_price_opt {
                    None => {
                        // First tick — record the baseline price; NAV unchanged.
                        token.last_rebalance_price = Some(current_price);
                        tracing::debug!(
                            lt_symbol = %symbol,
                            baseline_price = %current_price,
                            "lt_nav_updater: baseline price initialised"
                        );
                    }
                    Some(last_price) if last_price > rust_decimal::Decimal::ZERO => {
                        let price_return = (current_price - last_price) / last_price;
                        let nav_multiplier = rust_decimal::Decimal::ONE
                            + rust_decimal::Decimal::from(leverage) * price_return;

                        // Guard against a negative NAV multiplier (extreme move).
                        if nav_multiplier > rust_decimal::Decimal::ZERO {
                            let new_nav = old_nav * nav_multiplier;
                            token.nav = new_nav;
                            tracing::debug!(
                                lt_symbol = %symbol,
                                old_nav = %old_nav,
                                new_nav = %new_nav,
                                price_return = %price_return,
                                "lt_nav_updater: NAV updated"
                            );
                        } else {
                            tracing::warn!(
                                lt_symbol = %symbol,
                                nav_multiplier = %nav_multiplier,
                                "lt_nav_updater: NAV multiplier ≤ 0 (extreme move), NAV unchanged"
                            );
                        }
                        token.last_rebalance_price = Some(current_price);
                    }
                    _ => {
                        // Degenerate last_price (zero or negative) — just update baseline.
                        token.last_rebalance_price = Some(current_price);
                    }
                }
            }
    }
}

// ---------------------------------------------------------------------------
// Copy trading settlement task
// ---------------------------------------------------------------------------

/// Settle copy trading profit share every hour.
///
/// For every active follow relation that has accumulated positive realised
/// PnL since the last settlement, calculate the master's performance fee
/// and transfer it from the follower's account to the master's account.
///
/// Only positive PnL is subject to the fee (losses are not shared).
/// The relation's `total_pnl` is reset to zero after each settlement so
/// the next period starts fresh.
async fn run_copy_trading_settlement(exchange: Arc<Exchange>) {
    let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(3600));
    // Offset the first tick so settlement does not happen at startup.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        tracing::info!("copy_trading_settlement: starting hourly settlement run");

            // Collect all active follow relations and the master's profit_share_rate.
            let relations: Vec<(uuid::Uuid, uuid::Uuid, uuid::Uuid, rust_decimal::Decimal, rust_decimal::Decimal)> = {
                let followers = exchange.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());
                let masters = exchange.copy_trading_masters.lock().unwrap_or_else(|e| e.into_inner());

                followers
                    .all_active_relations()
                    .into_iter()
                    .filter_map(|r| {
                        if r.total_pnl <= rust_decimal::Decimal::ZERO {
                            return None; // no profit to share
                        }
                        let rate = masters
                            .get(r.master_id)
                            .map(|m| m.profit_share_rate)
                            .unwrap_or(rust_decimal::Decimal::ZERO);
                        if rate <= rust_decimal::Decimal::ZERO {
                            return None; // master takes no fee
                        }
                        Some((r.id, r.follower_id, r.master_id, r.total_pnl, rate))
                    })
                    .collect()
            };

            if relations.is_empty() {
                tracing::debug!("copy_trading_settlement: no relations with positive PnL");
                continue;
            }

            let mut settled = 0u32;
            for (relation_id, follower_id, master_id, gross_pnl, share_rate) in relations {
                match exchange_copy_trading::settlement::calculate_profit_share(gross_pnl, share_rate) {
                    Ok((master_share, _follower_net)) if master_share > rust_decimal::Decimal::ZERO => {
                        // Transfer master_share from follower to master.
                        // S18 Phase B: atomic 2-shard transfer with rollback on failure.
                        let transfer_ok = exchange
                            .accounts
                            .transfer(follower_id, master_id, "USDT", master_share)
                            .is_ok();

                        if transfer_ok {
                            // Reset the relation PnL so the next period starts from zero.
                            {
                                let mut followers = exchange.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());
                                followers.reset_pnl(relation_id);
                            }
                            settled += 1;
                            tracing::info!(
                                relation_id = %relation_id,
                                follower_id = %follower_id,
                                master_id = %master_id,
                                gross_pnl = %gross_pnl,
                                master_share = %master_share,
                                "copy_trading_settlement: profit share transferred"
                            );
                        } else {
                            tracing::warn!(
                                relation_id = %relation_id,
                                follower_id = %follower_id,
                                master_share = %master_share,
                                "copy_trading_settlement: follower insufficient balance for profit share"
                            );
                        }
                    }
                    Ok(_) => {} // zero share
                    Err(e) => {
                        tracing::error!(
                            relation_id = %relation_id,
                            error = %e,
                            "copy_trading_settlement: calculate_profit_share error"
                        );
                    }
                }
            }

            tracing::info!(settled = settled, "copy_trading_settlement: run complete");
    }
}

// ---------------------------------------------------------------------------
// P2P order timeout checker
// ---------------------------------------------------------------------------

/// Auto-cancel P2P orders that have been in the `Created` state for more
/// than 30 minutes without the buyer marking payment as sent.
///
/// When an order times out:
/// 1. Its status is set to `Cancelled`.
/// 2. The escrowed crypto is returned to the seller's account.
pub fn spawn_p2p_timeout_checker(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("p2p_timeout_checker", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            ticker.tick().await;

            let timeout_threshold = chrono::Utc::now() - chrono::Duration::minutes(30);

            // Identify timed-out orders without holding the lock across the
            // account mutation.
            let timed_out: Vec<(uuid::Uuid, uuid::Uuid, String, rust_decimal::Decimal)> = {
                let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
                let mut result = Vec::new();
                for order in orders.iter_mut() {
                    if order.status == crate::exchange::P2pOrderStatus::Created
                        && order.created_at < timeout_threshold
                    {
                        order.status = crate::exchange::P2pOrderStatus::Cancelled;
                        order.updated_at = chrono::Utc::now();
                        result.push((
                            order.id,
                            order.seller_id,
                            order.asset.clone(),
                            order.crypto_amount,
                        ));
                    }
                }
                result
            };

            for (order_id, seller_id, asset, crypto_amount) in timed_out {
                // Return escrowed crypto to seller.
                let _ = exchange.accounts.deposit(seller_id, &asset, crypto_amount);
                tracing::info!(
                    order_id = %order_id,
                    seller_id = %seller_id,
                    asset = %asset,
                    amount = %crypto_amount,
                    "p2p_timeout_checker: order auto-cancelled, escrow returned to seller"
                );
            }
            exchange.metrics.touch_task_heartbeat("p2p_timeout_checker");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// Mark price broadcaster
// ---------------------------------------------------------------------------

/// Broadcast [`MarketEvent::MarkPriceUpdate`] for every active perpetual symbol
/// every 3 seconds.
///
/// Clients subscribe to `markPrice.{symbol}` to receive real-time mark and
/// index price updates without waiting for a trade to occur.
pub fn spawn_mark_price_broadcaster(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("mark_price_broadcaster", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(3));
        loop {
            ticker.tick().await;

            let mark_prices = exchange.get_mark_prices();
            let now_ms = chrono::Utc::now().timestamp_millis();

            for (symbol, mp) in &mark_prices {
                let index_price = {
                    let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                    match agg.calculate_index(symbol) {
                        Ok(ip) => ip.price,
                        Err(_) => mp.price, // fall back to mark price if no index
                    }
                };

                let funding_rate = {
                    let history = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());
                    history
                        .get(symbol)
                        .and_then(|h| h.latest_rate())
                        .map(|r| r.rate)
                        .unwrap_or(rust_decimal::Decimal::ZERO)
                };

                let _ = exchange.market_tx.send(MarketEvent::MarkPriceUpdate {
                    symbol: symbol.to_string(),
                    mark_price: mp.price,
                    index_price,
                    funding_rate,
                    timestamp: now_ms,
                });
            }
            exchange.metrics.touch_task_heartbeat("mark_price_broadcaster");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// ADL warning broadcaster — emit AdlWarning for top-quantile positions
// ---------------------------------------------------------------------------

/// Scan all open positions every 30 seconds and broadcast
/// [`PrivateEvent::AdlWarning`] for any position in quantile 4 or 5.
///
/// This gives the user advance notice before their position is auto-delevered.
/// The warning is sent on the private `adl.{symbol}` channel.
pub fn spawn_adl_warning_broadcaster(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("adl_warning_broadcaster", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;

            let mark_prices_full = exchange.get_mark_prices();
            if mark_prices_full.is_empty() {
                exchange.metrics.touch_task_heartbeat("adl_warning_broadcaster");
                continue;
            }

            let all_positions: Vec<exchange_common::futures::Position> = {
                let pm = exchange.positions.lock();
                pm.all_positions().into_iter().cloned().collect()
            };

            if all_positions.is_empty() {
                continue;
            }

            let now_ms = chrono::Utc::now().timestamp_millis();

            for pos in &all_positions {
                let mark = match mark_prices_full.get(&pos.symbol) {
                    Some(mp) => mp.price,
                    None => continue,
                };

                let quantile = exchange_risk_engine::adl_quantile(pos, &all_positions, mark);

                // Only warn for high-risk quantiles (4 and 5).
                if let Some(q) = quantile {
                    if q >= 4 {
                        let _ = exchange.private_tx.send(PrivateEvent::AdlWarning {
                            user_id: pos.user_id.to_string(),
                            symbol: pos.symbol.to_string(),
                            quantile: q,
                            side: format!("{:?}", pos.side),
                            size: pos.size,
                            mark_price: mark,
                            timestamp: now_ms,
                        });

                        tracing::debug!(
                            user_id = %pos.user_id,
                            symbol = %pos.symbol,
                            quantile = q,
                            "ADL warning broadcast",
                        );
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("adl_warning_broadcaster");
        }
        })
    })
}

/// Decay stale 30-day VIP volumes every hour.
///
/// Users who have not traded in the last 30 days will have their `volume_30d`
/// reset to zero, which may demote their VIP tier.  This prevents users from
/// retaining elevated fee tiers indefinitely on the basis of stale volume.
pub fn spawn_vip_volume_decay(exchange: Arc<crate::exchange::Exchange>) -> JoinHandle<()> {
    supervised_spawn("vip_volume_decay", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        // Run once per hour.
        let mut ticker = interval(Duration::from_secs(3600));

        loop {
            ticker.tick().await;

            let mut fee_engine = exchange.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
            fee_engine.decay_stale_volumes();
            tracing::debug!("vip_volume_decay: stale volume decay pass completed");
            exchange.metrics.touch_task_heartbeat("vip_volume_decay");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// Audit log DB flusher
// ---------------------------------------------------------------------------

/// Periodically flush in-memory audit log entries to the PostgreSQL `audit_log`
/// table, providing durable 7-year retention as required by regulatory policy.
///
/// # Design
///
/// The in-memory [`AuditLog`] is the fast, always-available write path.  This
/// task runs every 30 seconds and persists any entries that have been appended
/// since the last flush.  Entries are flushed strictly once: we track the last
/// flushed `id` and only write entries with `id > last_flushed_id`.
///
/// On restart the DB already contains the previously flushed entries.  New
/// in-memory entries start from `id = 1` again, so on the first post-restart
/// flush we rewrite only entries that are not yet in the DB.  Because the DB
/// uses `INSERT … ON CONFLICT DO NOTHING` there is no double-write risk.
///
/// # Trade-offs
///
/// - Up to 30 seconds of audit entries may be in-memory only (acceptable for
///   operational risk; catastrophic data loss requires a process crash *and* a
///   PostgreSQL failure within that window).
/// - For stricter durability the flush interval can be lowered or `AuditLog::log`
///   can be changed to call `AuditRepo::append` synchronously via a DB pool that
///   is always available.
pub fn spawn_audit_log_flusher(exchange: Arc<crate::exchange::Exchange>) -> JoinHandle<()> {
    supervised_spawn("audit_log_flusher", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(30));
        // Track the last in-memory id we have already written to the DB so we
        // only flush new entries on each tick.
        let mut last_flushed_id: u64 = 0;

        loop {
            ticker.tick().await;

            // Only flush when a DB pool is available.
            let pool = match exchange.db_pool.get().cloned() {
                Some(p) => p,
                None => continue,
            };

            // Snapshot entries with id > last_flushed_id without holding the
            // lock across an async boundary.
            let entries_to_flush: Vec<exchange_admin::AuditEntry> = {
                let log = exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner());
                log.entries_since_id(last_flushed_id)
                    .into_iter()
                    .cloned()
                    .collect()
            };

            if entries_to_flush.is_empty() {
                continue;
            }

            let mut flushed = 0usize;
            let mut new_last_id = last_flushed_id;

            for entry in &entries_to_flush {
                let action_str = format!("{:?}", entry.action);
                let ip = entry.ip_address.as_deref();

                // INSERT … ON CONFLICT DO NOTHING is handled by the repo's
                // RETURNING semantics — we silently skip already-persisted rows.
                match exchange_db::audit_repo::AuditRepo::append(
                    &pool,
                    entry.user_id,
                    &action_str,
                    entry.detail.clone(),
                    ip,
                    &entry.prev_hash,
                    &entry.entry_hash,
                )
                .await
                {
                    Ok(_) => {
                        flushed += 1;
                        new_last_id = new_last_id.max(entry.id);
                    }
                    Err(e) => {
                        tracing::warn!(
                            audit_id = entry.id,
                            error = %e,
                            "audit_log_flusher: failed to persist entry — will retry next tick"
                        );
                        // Stop flushing on the first error so we don't skip entries.
                        // The next tick will retry from new_last_id onward.
                        break;
                    }
                }
            }

            if flushed > 0 {
                tracing::debug!(flushed, last_id = new_last_id, "audit_log_flusher: flushed entries to DB");
                last_flushed_id = new_last_id;
            }
            exchange.metrics.touch_task_heartbeat("audit_log_flusher");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// Task heartbeat watchdog (P2-R1)
// ---------------------------------------------------------------------------

/// Maximum time (seconds) a task can go without a heartbeat before the
/// watchdog logs an error. Tasks that miss this deadline are considered hung.
const HEARTBEAT_TIMEOUT_SECS: u64 = 120;

/// Periodically scans all task heartbeats and logs an error for any task
/// that hasn't heartbeated within `HEARTBEAT_TIMEOUT_SECS`.
///
/// This addresses FMEA F10: a deadlocked or hung background task silently
/// stops doing work without panicking, so `supervised_spawn` never restarts
/// it. The watchdog detects this condition and makes it observable.
///
/// Phase 1: detection + logging. Phase 2: automatic restart of hung tasks
/// (requires refactoring `spawn_all_tasks` to return task handles keyed by name).
pub fn spawn_task_heartbeat_watchdog(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("heartbeat_watchdog", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(30));

            loop {
                ticker.tick().await;

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let heartbeats = exchange.metrics.all_task_heartbeats();

                for (task_name, last_beat) in &heartbeats {
                    let age = now.saturating_sub(*last_beat);
                    if age > HEARTBEAT_TIMEOUT_SECS {
                        tracing::error!(
                            task = %task_name,
                            last_heartbeat_secs_ago = age,
                            threshold = HEARTBEAT_TIMEOUT_SECS,
                            "WATCHDOG: task missed heartbeat deadline — possibly hung or deadlocked"
                        );
                    }
                }

                // The watchdog itself heartbeats so it appears in the dashboard.
                exchange.metrics.touch_task_heartbeat("heartbeat_watchdog");
            }
        })
    })
}

// ---------------------------------------------------------------------------
// Engine state snapshotter (P1-D3 part 2)
// ---------------------------------------------------------------------------

/// Periodically write a full-fidelity engine snapshot to disk.
///
/// Every 5 minutes, snapshots every symbol's order book (via the engine
/// command channel) and writes the result to `data/snapshots/engine_snapshot.json`.
/// The snapshot includes the current WAL sequence so startup can skip
/// replaying commands already covered by the snapshot.
pub fn spawn_engine_snapshotter(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("engine_snapshotter", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(300)); // 5 minutes
            let data_dir = std::env::var("EXCHANGE_DATA_DIR").unwrap_or_else(|_| "./data".to_string());
            let snap_dir = std::path::PathBuf::from(&data_dir).join("snapshots");
            let _ = std::fs::create_dir_all(&snap_dir);
            let snap_path = snap_dir.join("engine_snapshot.json");

            loop {
                ticker.tick().await;

                // Get the current WAL sequence from the journal.
                let wal_seq = {
                    let journal = exchange.journal.lock().unwrap_or_else(|e| e.into_inner());
                    journal.as_ref().map(|wal| wal.sequence()).unwrap_or(0)
                };

                let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                let books = pool.full_snapshots_all();
                drop(pool);

                if books.is_empty() {
                    exchange.metrics.touch_task_heartbeat("engine_snapshotter");
                    continue;
                }

                let snapshot = crate::engine_snapshot::EngineStateSnapshot {
                    wal_sequence: wal_seq,
                    books,
                    created_at: chrono::Utc::now().to_rfc3339(),
                };

                match crate::engine_snapshot::write_snapshot(&snap_path, &snapshot) {
                    Ok(()) => {
                        let total_orders: usize = snapshot.books.iter().map(|b| b.orders.len()).sum();
                        tracing::info!(
                            symbols = snapshot.books.len(),
                            total_orders,
                            path = %snap_path.display(),
                            "Engine snapshot written"
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Engine snapshot write FAILED");
                    }
                }

                exchange.metrics.touch_task_heartbeat("engine_snapshotter");
            }
        })
    })
}

// ---------------------------------------------------------------------------
// Options settlement
// ---------------------------------------------------------------------------

/// Background task: cash-settle expired option contracts.
///
/// Runs every 60 seconds.  For each active instrument whose `expiry` has
/// passed the task:
/// 1. Computes intrinsic value at expiry:
///    - Call: max(0, spot − strike)
///    - Put:  max(0, strike − spot)
/// 2. Credits long holders and debits short holders accordingly.
/// 3. Zeros out all positions in that instrument.
/// 4. Marks the instrument inactive so it disappears from the chain.
pub fn spawn_options_settlement(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("options_settlement", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Collect expired active instruments (snapshot to avoid holding the lock).
            let expired: Vec<crate::exchange::OptionInstrument> = {
                let guard = exchange.option_instruments.lock().unwrap_or_else(|e| e.into_inner());
                guard
                    .iter()
                    .filter(|i| i.is_active && i.expiry <= now)
                    .cloned()
                    .collect()
            };

            for inst in &expired {
                let spot = exchange
                    .get_mark_price(&exchange_common::Symbol::new(&inst.underlying))
                    .map(|mp| mp.price)
                    .unwrap_or(inst.strike_price);

                let intrinsic = match inst.option_type {
                    crate::exchange::OptionType::Call => (spot - inst.strike_price).max(Decimal::ZERO),
                    crate::exchange::OptionType::Put  => (inst.strike_price - spot).max(Decimal::ZERO),
                };

                tracing::info!(
                    symbol    = %inst.symbol,
                    spot      = %spot,
                    strike    = %inst.strike_price,
                    intrinsic = %intrinsic,
                    "options settlement: instrument expired"
                );

                // Snapshot positions to settle.
                let to_settle: Vec<(Uuid, Decimal)> = {
                    let guard = exchange.option_positions.lock().unwrap_or_else(|e| e.into_inner());
                    guard
                        .iter()
                        .filter(|((_, sym), pos)| sym == &inst.symbol && pos.qty != Decimal::ZERO)
                        .map(|((uid, _), pos)| (*uid, pos.qty))
                        .collect()
                };

                for (user_id, qty) in to_settle {
                    // payout = intrinsic × qty × contract_size
                    // Long (qty > 0) → receive; Short (qty < 0) → pay.
                    let payout = intrinsic * qty * inst.contract_size;

                    // (paid, unpaid) — how much was actually debited and how
                    // much remained uncovered. All balance math runs under one
                    // shard lock via `with_user`.
                    let (actual_paid, unpaid) = exchange.accounts.with_user(user_id, |accounts| {
                        if payout > Decimal::ZERO {
                            let _ = accounts.deposit(user_id, "USDT", payout);
                            (Decimal::ZERO, Decimal::ZERO)
                        } else if payout < Decimal::ZERO {
                            let debt = payout.abs();
                            let available = accounts
                                .get_balance(&user_id, "USDT")
                                .map(|b| b.available())
                                .unwrap_or_default();
                            let actual = debt.min(available);
                            if actual > Decimal::ZERO {
                                let _ = accounts.withdraw(user_id, "USDT", actual);
                            }
                            (actual, debt - actual)
                        } else {
                            (Decimal::ZERO, Decimal::ZERO)
                        }
                    });

                    if payout < Decimal::ZERO && unpaid > Decimal::ZERO {
                        let debt = payout.abs();
                        tracing::error!(
                            user_id = %user_id,
                            symbol = %inst.symbol,
                            debt = %debt,
                            paid = %actual_paid,
                            unpaid = %unpaid,
                            "CRITICAL: options settlement debt — user cannot cover obligation"
                        );
                        // Draw from insurance fund for this symbol's underlying.
                        let underlying_sym = exchange_common::Symbol::new(format!("{}USDT", inst.underlying));
                        let mut ins_funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(fund) = ins_funds.get_mut(&underlying_sym) {
                            let covered = unpaid.min(fund.balance());
                            if covered > Decimal::ZERO {
                                let _ = fund.cover_deficit(covered);
                                tracing::warn!(
                                    symbol = %inst.symbol,
                                    covered = %covered,
                                    "options settlement debt covered by insurance fund"
                                );
                            }
                        }
                    }

                    tracing::info!(
                        user_id = %user_id,
                        symbol  = %inst.symbol,
                        qty     = %qty,
                        payout  = %payout,
                        "options cash settlement applied"
                    );

                    // Zero out position.
                    let mut positions = exchange.option_positions.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(pos) = positions.get_mut(&(user_id, inst.symbol.clone())) {
                        pos.qty            = Decimal::ZERO;
                        pos.unrealised_pnl = Decimal::ZERO;
                        pos.updated_at     = now;
                    }
                }

                // Deactivate the instrument.
                {
                    let mut guard = exchange.option_instruments.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(i) = guard.iter_mut().find(|i| i.symbol == inst.symbol) {
                        i.is_active = false;
                    }
                }
            }
            exchange.metrics.touch_task_heartbeat("options_settlement");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// Delivery futures settlement
// ---------------------------------------------------------------------------

/// Background task: settle expired delivery (quarterly) futures contracts.
///
/// Runs every 60 seconds.  For each `Active` contract whose `delivery_date`
/// has passed the task:
/// 1. Marks the contract `Settling`.
/// 2. Snapshots the index / mark price as the settlement price.
/// 3. PnL = (settlement_price − avg_entry) × qty × contract_size.
/// 4. Returns initial margin (10 %) plus PnL to each user.
/// 5. Zeros out all positions in the contract.
/// 6. Records the settlement price and marks the contract `Delivered`.
pub fn spawn_delivery_settlement(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("delivery_settlement", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = Utc::now();

            // Contracts past their delivery date that are still Active.
            let due: Vec<crate::exchange::DeliveryContract> = {
                let guard = exchange.delivery_contracts.lock().unwrap_or_else(|e| e.into_inner());
                guard
                    .iter()
                    .filter(|c| {
                        c.delivery_date <= now
                            && matches!(c.status, crate::exchange::DeliveryStatus::Active)
                    })
                    .cloned()
                    .collect()
            };

            for contract in &due {
                // Step 1: transition to Settling.
                {
                    let mut guard = exchange.delivery_contracts.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(c) = guard.iter_mut().find(|c| c.symbol == contract.symbol) {
                        c.status = crate::exchange::DeliveryStatus::Settling;
                    }
                }

                // Step 2: snapshot settlement price.
                let settlement_price = exchange
                    .get_mark_price(&exchange_common::Symbol::new(&contract.underlying))
                    .map(|mp| mp.price)
                    .unwrap_or(Decimal::ZERO);

                tracing::info!(
                    symbol           = %contract.symbol,
                    settlement_price = %settlement_price,
                    "delivery settlement: contract settling"
                );

                // Step 3-4: collect and settle positions.
                let to_settle: Vec<(Uuid, Decimal, Decimal)> = {
                    let guard = exchange.delivery_positions.lock().unwrap_or_else(|e| e.into_inner());
                    guard
                        .iter()
                        .filter(|((_, sym), pos)| sym == &contract.symbol && pos.qty != Decimal::ZERO)
                        .map(|((uid, _), pos)| (*uid, pos.qty, pos.avg_entry_price))
                        .collect()
                };

                for (user_id, qty, avg_entry) in to_settle {
                    let pnl            = (settlement_price - avg_entry) * qty * contract.contract_size;
                    let margin_back    = avg_entry * qty.abs() * contract.contract_size / Decimal::new(10, 0);
                    let total_credit   = margin_back + pnl;

                    if total_credit > Decimal::ZERO {
                        let _ = exchange.accounts.deposit(user_id, "USDT", total_credit);
                    } else {
                        tracing::warn!(
                            user_id = %user_id,
                            symbol  = %contract.symbol,
                            pnl     = %pnl,
                            "delivery settlement: position loss exceeds margin"
                        );
                    }

                    tracing::info!(
                        user_id          = %user_id,
                        symbol           = %contract.symbol,
                        qty              = %qty,
                        settlement_price = %settlement_price,
                        pnl              = %pnl,
                        total_credit     = %total_credit,
                        "delivery position settled"
                    );

                    // Step 5: zero out position.
                    let mut positions = exchange.delivery_positions.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(pos) = positions.get_mut(&(user_id, contract.symbol.clone())) {
                        pos.qty            = Decimal::ZERO;
                        pos.unrealised_pnl = Decimal::ZERO;
                        pos.updated_at     = now;
                    }
                }

                // Step 6: record history and mark Delivered.
                {
                    let mut history = exchange
                        .delivery_settlement_history
                        .lock().unwrap_or_else(|e| e.into_inner());
                    crate::exchange::capped_push(
                        &mut history,
                        crate::exchange::DeliverySettlementRecord {
                            symbol:           contract.symbol.clone(),
                            underlying:       contract.underlying.clone(),
                            settlement_price,
                            settled_at:       now,
                        },
                        crate::exchange::MAX_DELIVERY_SETTLEMENT_HISTORY,
                    );
                }
                {
                    let mut guard = exchange.delivery_contracts.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(c) = guard.iter_mut().find(|c| c.symbol == contract.symbol) {
                        c.status = crate::exchange::DeliveryStatus::Delivered;
                    }
                }

                tracing::info!(
                    symbol           = %contract.symbol,
                    settlement_price = %settlement_price,
                    "delivery settlement complete"
                );
            }
            exchange.metrics.touch_task_heartbeat("delivery_settlement");
        }
        })
    })
}

// =============================================================================
// Feature 1 — Daily Account Snapshot
// =============================================================================

/// Spawn the daily account snapshot background task.
///
/// Runs every hour. When the current UTC time is midnight (00:00 – 00:59),
/// takes a snapshot of every known user's equity and writes it to
/// `exchange.daily_snapshots`.
pub fn spawn_daily_snapshot(exchange: std::sync::Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("daily_snapshot", move || {
        let ex = std::sync::Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(3600)); // hourly
            loop {
                ticker.tick().await;
                let now = chrono::Utc::now();
                // Only take the snapshot during the first hour of the day (midnight window).
                if now.format("%H").to_string() != "00" {
                    tracing::trace!("daily_snapshot: not midnight, skipping");
                    continue;
                }

                let date_str = now.format("%Y-%m-%d").to_string();
                tracing::info!(date = %date_str, "taking daily account snapshots");

                // Collect all known user IDs from the accounts manager.
                let user_ids: Vec<uuid::Uuid> = ex.accounts.user_ids();

                for user_id in user_ids {
                    // Skip if a snapshot for this user+date already exists
                    // (prevents double-writes on hourly re-runs within the window).
                    {
                        let snaps = ex.daily_snapshots.lock().unwrap_or_else(|e| e.into_inner());
                        if snaps.contains_key(&(user_id, date_str.clone())) {
                            continue;
                        }
                    }

                    // Build balance map for this user (available balances only).
                    let balances: std::collections::HashMap<String, Decimal> = ex
                        .accounts
                        .get_all_balances(user_id)
                        .iter()
                        .map(|(k, v)| (k.clone(), v.available()))
                        .collect();

                    let total_usdt = balances.get("USDT").copied().unwrap_or(Decimal::ZERO);

                    // Unrealised PnL from open futures positions.
                    // We need mark prices to compute; fall back to entry price (zero PnL)
                    // if marks are unavailable for a symbol.
                    let (unrealised_pnl, positions_count) = {
                        let mark_prices = ex.get_mark_prices();
                        let pm = ex.positions.lock();
                        let user_positions = pm.user_positions(user_id);
                        let count = user_positions.len();
                        let pnl = user_positions.iter().fold(Decimal::ZERO, |acc, p| {
                            let mark = mark_prices
                                .get(&p.symbol)
                                .map(|mp| mp.price)
                                .unwrap_or(p.entry_price);
                            acc + p.unrealized_pnl(mark)
                        });
                        (pnl, count)
                    };

                    let snapshot = crate::exchange::DailySnapshot {
                        user_id,
                        date: date_str.clone(),
                        total_equity_usd: total_usdt + unrealised_pnl,
                        balances,
                        positions_count,
                        unrealised_pnl,
                        created_at: now,
                    };

                    ex.daily_snapshots
                        .lock().unwrap_or_else(|e| e.into_inner())
                        .insert((user_id, date_str.clone()), snapshot);
                }

                tracing::info!(date = %date_str, "daily snapshots complete");
            }
        })
    })
}

// =============================================================================
// Feature 6 — Premium Snapshot (runs alongside OI snapshotter)
// =============================================================================

/// Spawn the premium-index snapshot task.
///
/// Every 5 minutes, computes `(mark_price - index_price) / index_price * 100`
/// for every active symbol and appends a [`PremiumSnapshot`] to
/// `exchange.premium_history`. History is capped at
/// [`MAX_PREMIUM_HISTORY_PER_SYMBOL`] entries per symbol.
pub fn spawn_premium_snapshotter(exchange: std::sync::Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("premium_snapshotter", move || {
        let ex = std::sync::Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(300)); // 5 minutes
            loop {
                ticker.tick().await;
                let now_ts = chrono::Utc::now().timestamp();

                // Get all mark prices (which include both mark and index).
                let mark_prices = ex.get_mark_prices();
                if mark_prices.is_empty() {
                    continue;
                }

                let mut history = ex.premium_history.lock().unwrap_or_else(|e| e.into_inner());
                for (symbol, mp) in &mark_prices {
                    if mp.index_price == Decimal::ZERO {
                        continue;
                    }
                    let premium_pct = (mp.price - mp.index_price)
                        / mp.index_price
                        * Decimal::new(100, 0);

                    let snap = crate::exchange::PremiumSnapshot {
                        timestamp: now_ts,
                        premium_pct,
                        mark_price: mp.price,
                        index_price: mp.index_price,
                    };

                    let deque = history.entry(symbol.clone()).or_default();
                    if deque.len() >= crate::exchange::MAX_PREMIUM_HISTORY_PER_SYMBOL {
                        deque.pop_front();
                    }
                    deque.push_back(snap);
                }
            }
        })
    })
}

// =============================================================================
// Feature 7 — All-Market Mini Ticker Broadcast
// =============================================================================

/// Spawn the all-market mini ticker broadcast task.
///
/// Every second, broadcasts a single [`MarketEvent::AllMiniTicker`] containing
/// a compact snapshot for every active symbol. Clients subscribe to the
/// `ticker.all` WS topic.
pub fn spawn_all_mini_ticker(exchange: std::sync::Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("all_mini_ticker", move || {
        let ex = std::sync::Arc::clone(&exchange);
        Box::pin(async move {
            let mut ticker = interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;

                let tickers: Vec<crate::exchange::AllMiniTickerEntry> = ex
                    .market_data
                    .all_tickers()
                    .into_iter()
                    .map(|t| crate::exchange::AllMiniTickerEntry {
                        symbol: t.symbol.to_string(),
                        price: t.last_price.to_string(),
                        change_24h: t.price_change_pct_24h.to_string(),
                    })
                    .collect();

                if tickers.is_empty() {
                    continue;
                }

                let event = MarketEvent::AllMiniTicker {
                    tickers,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };

                if let Err(e) = ex.market_tx.send(event) {
                    tracing::trace!("all_mini_ticker: no WS subscribers ({})", e);
                }
            }
        })
    })
}

// ---------------------------------------------------------------------------
// Surveillance auto-action enforcement (M19-1/2/3)
// ---------------------------------------------------------------------------

/// Periodically run all surveillance detectors on recently active users and
/// enforce auto-actions (CancelOrders, FreezeAccount, ReduceRateLimit).
///
/// Interval: every 5 seconds. Collects the set of users who placed/cancelled
/// orders since the last tick from the surveillance engine, runs `check_all`,
/// and calls `enforce_auto_actions` on any resulting alerts.
pub fn spawn_surveillance_monitor(exchange: Arc<Exchange>) -> JoinHandle<()> {
    supervised_spawn("surveillance_monitor", move || {
        let exchange = Arc::clone(&exchange);
        Box::pin(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;

            let now = chrono::Utc::now();

            // Collect active users from recent pending orders (snapshot).
            let active_users: Vec<(uuid::Uuid, Option<exchange_common::types::Symbol>)> = exchange
                .pending_orders
                .iter()
                .map(|r| (r.user_id, Some(r.symbol.clone())))
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            if active_users.is_empty() {
                continue;
            }

            // Run all detectors.
            let alerts = {
                let mut surv = exchange
                    .surveillance
                    .lock();
                surv.check_all(&active_users, now)
            };

            if !alerts.is_empty() {
                tracing::info!(
                    alert_count = alerts.len(),
                    "surveillance: {} new alerts, enforcing auto-actions",
                    alerts.len()
                );
                exchange.enforce_auto_actions(&alerts);
            }
            exchange.metrics.touch_task_heartbeat("surveillance_monitor");
        }
        })
    })
}

// ---------------------------------------------------------------------------
// S15: Post-fill event worker (B.3)
// ---------------------------------------------------------------------------

/// Spawn the background task that processes ASYNC-SAFE post-fill side effects.
///
/// The worker drains [`PostFillEvent`] messages from the bounded MPSC channel
/// stored in `exchange.post_fill_event_rx`.  For each `PostFillEvent::Fill` it
/// calls `handle_fill_side_effects` (surveillance, notifications, WS broadcast,
/// FIX drop-copy, trade history).  For `PostFillEvent::OrderAccepted` it emits
/// the private WS `OrderAccepted` event.
///
/// **Design constraints (B.5):**
/// - The channel uses `try_send` on the producer side — this task is the sole
///   consumer.
/// - If the task falls behind, `try_send` will return `Full` and events will be
///   dropped with a counter increment.  The hot path is NEVER blocked.
/// - The task is supervised: it restarts on panic after a 1-second back-off.
///
/// **Ordering note:** FIFO within the channel is preserved (tokio mpsc is
/// ordered).  However, multiple fills from the same order can arrive as
/// separate events and will be processed sequentially, so WS delivery order
/// matches fill order.  User devices may still receive pushes slightly
/// out-of-order if the push provider queues them on its side.
pub fn spawn_post_fill_event_worker(exchange: Arc<Exchange>) -> JoinHandle<()> {
    // Take the receiver out of the Option<Mutex<Receiver>> — this can only be
    // done once.  Subsequent calls (or test teardown) will find None and log a
    // warning rather than panicking.
    let rx_opt = exchange
        .post_fill_event_rx
        .lock().unwrap_or_else(|e| e.into_inner())
        .take();

    let Some(mut rx) = rx_opt else {
        tracing::warn!("spawn_post_fill_event_worker: receiver already taken — worker not started");
        return tokio::spawn(async {});
    };

    tokio::spawn(async move {
        tracing::info!("post_fill_event_worker started");
        while let Some(event) = rx.recv().await {
            match event {
                PostFillEvent::Fill { fill, is_futures } => {
                    // All ASYNC-SAFE side effects for a fill.
                    exchange.handle_fill_side_effects(&fill, is_futures);
                }
                PostFillEvent::OrderAccepted { order_id, user_id, symbol } => {
                    // Emit private WS event that was deferred from process_events_sync.
                    if exchange.private_tx.send(PrivateEvent::OrderAccepted {
                        user_id: user_id.to_string(),
                        order_id: order_id.to_string(),
                        symbol: symbol.to_string(),
                    }).is_err() {
                        tracing::debug!("private broadcast: no active subscribers");
                    }
                    // Broadcast the order-book snapshot for this symbol.
                    if let Ok(snap) = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner()).snapshot(&symbol, 25) {
                        if exchange.market_tx.send(MarketEvent::BookUpdate {
                            symbol: symbol.to_string(),
                            bids: snap.bids,
                            asks: snap.asks,
                            sequence: snap.sequence,
                        }).is_err() {
                            tracing::debug!("market broadcast: no active subscribers");
                        }
                    }
                }
                PostFillEvent::OrderCancelled { order_id, user_id, symbol, remaining_qty } => {
                    // Private WS cancellation notification (already handled inside
                    // handle_cancel for the balance-unlock path; this variant is
                    // reserved for future use when we want to decouple cancel
                    // notifications from the sync cancel path).
                    let _ = (order_id, user_id, symbol, remaining_qty);
                }
            }
        }
        tracing::info!("post_fill_event_worker exited — channel closed");
    })
}


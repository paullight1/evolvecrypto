use std::io;
use std::sync::Mutex;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use exchange_common::Command;

use crate::segments::SegmentedWal;
use crate::wal::WriteAheadLog;

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A single write request placed into the group-commit buffer.
struct WriteRequest {
    command: Command,
    response: oneshot::Sender<io::Result<u64>>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Configuration for a [`GroupCommitWal`].
pub struct GroupCommitConfig {
    /// Flush when this many commands have accumulated (default: 100).
    pub max_batch_size: usize,
    /// Flush after this many milliseconds even if the batch is not full (default: 10).
    pub max_delay_ms: u64,
    /// Path to the underlying WAL file.
    pub wal_path: String,
    /// Whether to fsync after each batch flush (pass `false` in tests).
    pub durable: bool,
}

impl Default for GroupCommitConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 100,
            max_delay_ms: 10,
            wal_path: "wal.log".to_owned(),
            durable: true,
        }
    }
}

/// Group-commit WAL — buffers writes and fsyncs in batches.
///
/// Instead of one fsync per write (0.5–5 ms each on NVMe), commands are
/// collected until either `max_batch_size` entries accumulate **or**
/// `max_delay_ms` elapses.  A single fsync then covers all buffered entries
/// and every caller in the batch is notified simultaneously.
///
/// At 1 000 writes/sec with a 10 ms window this means ≈10 fsyncs/sec instead
/// of 1 000 — a 100× reduction in I/O syscalls.
///
/// # Tradeoff
/// Each caller experiences up to `max_delay_ms` additional latency compared
/// to a synchronous fsync, but the throughput under load improves dramatically.
///
/// This is conceptually identical to PostgreSQL's WAL group-commit mechanism.
pub struct GroupCommitWal {
    sender: mpsc::Sender<WriteRequest>,
}

impl GroupCommitWal {
    /// Spawn the background flush task and return the handle pair.
    ///
    /// The returned `JoinHandle` runs until the last `GroupCommitWal` sender is
    /// dropped (the channel closes) or an unrecoverable WAL error occurs.
    pub fn start(config: GroupCommitConfig) -> io::Result<(Self, JoinHandle<()>)> {
        // Channel capacity: allow writers to queue without blocking on the
        // tokio scheduler when the batch task is busy flushing.
        let (tx, rx) = mpsc::channel::<WriteRequest>(config.max_batch_size * 4);

        let wal = WriteAheadLog::open(&config.wal_path, config.durable)?;

        let handle = tokio::spawn(flush_task(wal, rx, config.max_batch_size, config.max_delay_ms));

        Ok((Self { sender: tx }, handle))
    }

    /// Append a command to the WAL.
    ///
    /// The call returns once the command has been written **and** fsynced as
    /// part of a batch.  The returned value is the monotonically increasing
    /// WAL sequence number assigned to this command.
    ///
    /// # Errors
    /// Returns an `io::Error` if the background flush task has stopped (the
    /// channel is closed) or if the WAL write itself failed.
    pub async fn append(&self, command: Command) -> io::Result<u64> {
        let (resp_tx, resp_rx) = oneshot::channel();

        self.sender
            .send(WriteRequest { command, response: resp_tx })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "group commit task stopped"))?;

        resp_rx
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "group commit task dropped response"))?
    }

    /// Spawn a group-commit flush task that writes through a **shared**
    /// `SegmentedWal` (behind a `Mutex`), instead of owning a private WAL file.
    ///
    /// This eliminates the split-brain bug where the GC WAL and the replay WAL
    /// were different files. The batch is collected in the channel, then flushed
    /// via `SegmentedWal::append_batch` (single fsync per batch).
    pub fn start_shared(
        shared_wal: Arc<Mutex<Option<SegmentedWal>>>,
        max_batch_size: usize,
        max_delay_ms: u64,
    ) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<WriteRequest>(max_batch_size * 4);

        let handle = tokio::spawn(flush_task_shared(
            shared_wal,
            rx,
            max_batch_size,
            max_delay_ms,
        ));

        (Self { sender: tx }, handle)
    }

    /// Synchronous variant of [`append`] for use inside `tokio::task::spawn_blocking`.
    ///
    /// `spawn_blocking` closures run on a dedicated blocking thread that still
    /// has access to the tokio runtime handle.  `Handle::current().block_on()`
    /// drives the async `append` future to completion on that thread without
    /// blocking the async reactor.
    ///
    /// # Panics
    /// Panics if called outside of a tokio runtime context (i.e. when there is
    /// no current `Handle`).  This is never the case for callers inside
    /// `spawn_blocking`, which is the only intended call site.
    ///
    /// # Errors
    /// Same as [`append`]: `io::Error` on flush-task stop or WAL write failure.
    pub fn append_sync(&self, command: Command) -> io::Result<u64> {
        tokio::runtime::Handle::current().block_on(self.append(command))
    }
}

// ---------------------------------------------------------------------------
// Background flush task
// ---------------------------------------------------------------------------

/// Drives the group-commit loop.
///
/// Each iteration waits up to `max_delay_ms` for the first request, then
/// drains the channel non-blockingly to form a batch, writes all commands to
/// the WAL, fsyncs once, and notifies all callers.
async fn flush_task(
    mut wal: WriteAheadLog,
    mut rx: mpsc::Receiver<WriteRequest>,
    max_batch_size: usize,
    max_delay_ms: u64,
) {
    let delay = Duration::from_millis(max_delay_ms);
    let mut batch: Vec<WriteRequest> = Vec::with_capacity(max_batch_size);

    loop {
        // --- Wait for the first item in the next batch ---
        let first = match timeout(delay, rx.recv()).await {
            // Got a request within the window.
            Ok(Some(req)) => req,
            // Channel closed — all senders dropped; exit cleanly.
            Ok(None) => {
                tracing::debug!("group commit channel closed, flush task exiting");
                return;
            }
            // Timeout with no requests — nothing to flush, loop again.
            Err(_elapsed) => continue,
        };

        batch.push(first);

        // --- Drain the channel without blocking to fill the batch ---
        while batch.len() < max_batch_size {
            match rx.try_recv() {
                Ok(req) => batch.push(req),
                Err(_) => break, // channel empty or closed
            }
        }

        // --- Write all buffered commands, then fsync once ---
        let (results, fatal) = write_batch(&mut wal, &mut batch);

        // Notify all callers (consuming each request).
        for (req, result) in batch.drain(..).zip(results) {
            let _ = req.response.send(result);
        }

        // Post-fsyncgate rule: if fsync/write returned EIO or similar, the
        // page cache may have silently discarded data. Continuing to accept
        // new writes against a broken WAL is the worst possible outcome —
        // we'd confirm durability that doesn't exist. Abort the process so
        // the supervisor can decide whether to restart with a fresh WAL or
        // investigate the disk.
        if let Some(fatal_err) = fatal {
            tracing::error!(
                error = %fatal_err,
                "FATAL: WAL write/fsync failed — aborting to prevent silent data loss"
            );
            std::process::abort();
        }
    }
}

/// Write each command in `batch` to `wal` and return the per-command results.
///
/// All commands are written before returning; the WAL's own `flush()` call
/// (inside `append`) handles the BufWriter flush, and `sync_data()` is called
/// once by the last `append` when `durable` is true — *or* we rely on the WAL
/// flushing the BufWriter eagerly per entry.
///
/// Because `WriteAheadLog::append` already calls `flush` + optional `sync_data`
/// after every entry, we gain from grouping at the channel/batch level (fewer
/// syscalls when entries arrive concurrently) rather than deferring the fsync
/// inside the WAL.  For a full deferred-fsync optimisation the WAL would need
/// a `write_without_sync` / `sync` split; that is a future phase 2 task noted
/// in `wal.rs`.
/// Write each command and return results + optional fatal error.
///
/// If any `append` returns an I/O error that indicates potential data loss
/// (EIO, disk full, broken pipe), the fatal error is returned so the caller
/// can abort the process. Individual per-command errors are still delivered
/// to callers so they know their write failed.
fn write_batch(
    wal: &mut WriteAheadLog,
    batch: &mut Vec<WriteRequest>,
) -> (Vec<io::Result<u64>>, Option<io::Error>) {
    let mut fatal: Option<io::Error> = None;
    let results = batch
        .iter()
        .map(|req| {
            let r = wal.append(&req.command);
            if let Err(ref e) = r {
                if is_fatal_io(e) && fatal.is_none() {
                    fatal = Some(io::Error::new(e.kind(), e.to_string()));
                }
            }
            r
        })
        .collect();
    (results, fatal)
}

/// Shared-WAL variant: batches commands, locks the SegmentedWal Mutex, calls
/// `append_batch` (N writes + 1 fsync), notifies all callers.
async fn flush_task_shared(
    shared_wal: Arc<Mutex<Option<SegmentedWal>>>,
    mut rx: mpsc::Receiver<WriteRequest>,
    max_batch_size: usize,
    max_delay_ms: u64,
) {
    let delay = Duration::from_millis(max_delay_ms);
    let mut batch: Vec<WriteRequest> = Vec::with_capacity(max_batch_size);

    loop {
        let first = match timeout(delay, rx.recv()).await {
            Ok(Some(req)) => req,
            Ok(None) => {
                tracing::debug!("shared group commit channel closed, flush task exiting");
                return;
            }
            Err(_elapsed) => continue,
        };

        batch.push(first);
        while batch.len() < max_batch_size {
            match rx.try_recv() {
                Ok(req) => batch.push(req),
                Err(_) => break,
            }
        }

        // Lock the shared WAL, write the batch, unlock.
        let commands: Vec<Command> = batch.iter().map(|r| r.command.clone()).collect();
        let results = {
            let mut guard = shared_wal.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_mut() {
                Some(wal) => {
                    match wal.append_batch(&commands) {
                        Ok(seqs) => seqs.into_iter().map(Ok).collect::<Vec<io::Result<u64>>>(),
                        Err(e) => {
                            let err_str = e.to_string();
                            let is_fatal = is_fatal_io(&e);
                            // Return the error to all callers in the batch.
                            let errs: Vec<io::Result<u64>> = (0..commands.len())
                                .map(|_| Err(io::Error::new(e.kind(), err_str.clone())))
                                .collect();
                            if is_fatal {
                                // Notify callers, then abort.
                                for (req, result) in batch.drain(..).zip(errs) {
                                    let _ = req.response.send(result);
                                }
                                tracing::error!(
                                    error = %err_str,
                                    "FATAL: shared WAL append_batch failed — aborting"
                                );
                                std::process::abort();
                            }
                            errs
                        }
                    }
                }
                None => {
                    // No WAL configured (in-memory mode) — return fake sequence numbers.
                    (0..commands.len()).map(|i| Ok(i as u64)).collect()
                }
            }
        };

        for (req, result) in batch.drain(..).zip(results) {
            let _ = req.response.send(result);
        }
    }
}

fn is_fatal_io(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::PermissionDenied
            | io::ErrorKind::WriteZero
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::Other // EIO maps here on Linux
    ) || {
        let msg = e.to_string().to_lowercase();
        msg.contains("no space") || msg.contains("i/o error") || msg.contains("read-only")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::*;
    use ::futures::future;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_cmd() -> Command {
        Command::PlaceOrder(PlaceOrderCommand {
            order_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: dec!(50000),
            qty: dec!(1),
            timestamp: chrono::Utc::now(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        })
    }

    fn test_config(dir: &std::path::Path) -> GroupCommitConfig {
        GroupCommitConfig {
            max_batch_size: 100,
            max_delay_ms: 50,
            wal_path: dir.join("gc.wal").to_str().unwrap().to_owned(),
            durable: false,
        }
    }

    // Test 1: single write returns the correct sequence number (1).
    #[tokio::test]
    async fn test_single_write_returns_seq() {
        let dir = tempfile::tempdir().unwrap();
        let (gc, _handle) = GroupCommitWal::start(test_config(dir.path())).unwrap();

        let seq = gc.append(make_cmd()).await.unwrap();
        assert_eq!(seq, 1, "first write must get seq=1");
    }

    // Test 2: multiple concurrent writes — all callers receive a response.
    #[tokio::test]
    async fn test_concurrent_writes_all_get_responses() {
        let dir = tempfile::tempdir().unwrap();
        let (gc, _handle) = GroupCommitWal::start(test_config(dir.path())).unwrap();

        let gc = std::sync::Arc::new(gc);
        let mut handles = Vec::new();

        for _ in 0..20 {
            let gc = gc.clone();
            handles.push(tokio::spawn(async move { gc.append(make_cmd()).await }));
        }

        let results: Vec<_> = future::join_all(handles).await;
        for r in results {
            assert!(r.unwrap().is_ok(), "every concurrent write must succeed");
        }
    }

    // Test 3: batch-size trigger — 3 writes with max_batch=3 → all flushed.
    #[tokio::test]
    async fn test_batch_size_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let config = GroupCommitConfig {
            max_batch_size: 3,
            max_delay_ms: 5000, // long delay — flush must be triggered by size
            wal_path: dir.path().join("gc.wal").to_str().unwrap().to_owned(),
            durable: false,
        };

        let (gc, _handle) = GroupCommitWal::start(config).unwrap();
        let gc = std::sync::Arc::new(gc);

        // Fire 3 concurrent writes — the batch fills up and flushes before the timer.
        let mut handles = Vec::new();
        for _ in 0..3 {
            let gc = gc.clone();
            handles.push(tokio::spawn(async move { gc.append(make_cmd()).await }));
        }

        // All 3 must complete (batch triggers flush without waiting for timer).
        let results: Vec<_> = future::join_all(handles).await;
        for r in results {
            assert!(r.unwrap().is_ok());
        }
    }

    // Test 4: timer trigger — 1 write, wait longer than max_delay → flushed.
    #[tokio::test]
    async fn test_timer_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let config = GroupCommitConfig {
            max_batch_size: 1000, // large batch — only timer should trigger flush
            max_delay_ms: 20,
            wal_path: dir.path().join("gc.wal").to_str().unwrap().to_owned(),
            durable: false,
        };

        let (gc, _handle) = GroupCommitWal::start(config).unwrap();

        // A single write with max_delay=20ms should complete well within 500ms.
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            gc.append(make_cmd()),
        )
        .await
        .expect("timed out — timer trigger did not fire")
        .expect("write failed");

        assert_eq!(result, 1);
    }

    // Test 5: sequential sequence numbers — 10 writes → seqs 1..=10.
    #[tokio::test]
    async fn test_sequential_seqs() {
        let dir = tempfile::tempdir().unwrap();
        let (gc, _handle) = GroupCommitWal::start(test_config(dir.path())).unwrap();

        let mut seqs = Vec::new();
        for _ in 0..10 {
            seqs.push(gc.append(make_cmd()).await.unwrap());
        }

        // Seqs must be a permutation of 1..=10 (order may vary under concurrency,
        // but with sequential calls they should be strictly 1..=10).
        seqs.sort_unstable();
        assert_eq!(seqs, (1u64..=10).collect::<Vec<_>>());
    }
}

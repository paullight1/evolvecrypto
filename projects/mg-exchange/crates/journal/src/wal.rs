use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use exchange_common::Command;

use crate::reader::WalReader;

/// Write-Ahead Log — every command is durably persisted BEFORE entering the matching engine.
///
/// Format: one JSON line per command, prefixed with sequence number.
/// ```text
/// 1\t{"PlaceOrder":{...}}\n
/// 2\t{"CancelOrder":{...}}\n
/// ```
///
/// Design:
/// - Append-only (no updates, no deletes)
/// - fsync after each write (durability guarantee)
/// - Sequence numbers are monotonic and gap-free
/// - Replay produces identical matching engine state (deterministic)
///
/// Phase 1: single file, fsync per write.
/// Phase 2: segment rotation (new file every N entries or M bytes), async fsync batching.
pub struct WriteAheadLog {
    writer: BufWriter<File>,
    path: PathBuf,
    sequence: u64,
    /// If false, skip fsync (for tests/benchmarks).
    durable: bool,
}

impl WriteAheadLog {
    /// Open or create a WAL file. Resumes from last sequence number.
    pub fn open(path: impl AsRef<Path>, durable: bool) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Determine next sequence number by reading existing entries
        let sequence = if path.exists() {
            let reader = WalReader::open(&path)?;
            let mut max_seq = 0u64;
            for entry in reader {
                let (seq, _) = entry?;
                max_seq = max_seq.max(seq);
            }
            max_seq
        } else {
            0
        };

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        Ok(Self {
            writer: BufWriter::new(file),
            path,
            sequence,
            durable,
        })
    }

    /// Open or create a WAL file, seeding the sequence counter to at least
    /// `min_starting_seq`.
    ///
    /// Used by [`SegmentedWal`](crate::SegmentedWal) when creating a new
    /// segment so that sequence numbers remain globally monotonic across
    /// segment boundaries.
    pub fn open_with_starting_seq(
        path: impl AsRef<Path>,
        durable: bool,
        min_starting_seq: u64,
    ) -> io::Result<Self> {
        let mut wal = Self::open(path, durable)?;
        if wal.sequence < min_starting_seq {
            wal.sequence = min_starting_seq;
        }
        Ok(wal)
    }

    /// Append a command to the WAL. Returns the assigned sequence number.
    ///
    /// The command is durably persisted (fsync'd) before this returns.
    /// The caller can then safely send the command to the matching engine.
    pub fn append(&mut self, command: &Command) -> io::Result<u64> {
        self.sequence += 1;
        let seq = self.sequence;

        let json = serde_json::to_string(command)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Format: seq\tjson\n
        write!(self.writer, "{}\t{}\n", seq, json)?;
        self.writer.flush()?;

        if self.durable {
            self.writer.get_ref().sync_data()?;
        }

        Ok(seq)
    }

    /// Append a command WITHOUT calling fdatasync.
    ///
    /// The entry is serialized and flushed to the OS buffer cache but NOT
    /// durably committed to disk. Call [`sync`] separately after a batch.
    /// Used by `SegmentedWal::append_batch` for group-commit semantics.
    pub fn append_no_sync(&mut self, command: &Command) -> io::Result<u64> {
        self.sequence += 1;
        let seq = self.sequence;

        let json = serde_json::to_string(command)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        write!(self.writer, "{}\t{}\n", seq, json)?;
        self.writer.flush()?;

        Ok(seq)
    }

    /// Issue fdatasync to durably commit all buffered writes.
    pub fn sync(&mut self) -> io::Result<()> {
        self.writer.get_ref().sync_data()
    }

    /// Current sequence number (last written).
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Whether fsync is enabled for this WAL.
    pub fn durable(&self) -> bool {
        self.durable
    }

    /// Path to the WAL file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Truncate the WAL (for snapshot + compaction).
    /// After taking a matching engine snapshot, old WAL entries are no longer needed.
    pub fn truncate(&mut self) -> io::Result<()> {
        // Close current writer
        self.writer.flush()?;
        drop(std::mem::replace(
            &mut self.writer,
            BufWriter::new(File::create(&self.path)?),
        ));

        // Reset sequence (snapshot contains the state)
        // In production, you'd keep the sequence and just rotate to a new segment file
        self.sequence = 0;

        Ok(())
    }

    /// Flush any buffered writes to the OS page cache.
    ///
    /// The WAL already calls `flush()` after every `append()`, so this is a
    /// no-op in the normal case. It is exposed for callers that want an
    /// explicit durability checkpoint (e.g. graceful shutdown).
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    /// Number of bytes written to the WAL file.
    pub fn file_size(&self) -> io::Result<u64> {
        fs::metadata(&self.path).map(|m| m.len())
    }

    /// Replay entries from `path` that have sequence number strictly greater than `after_seq`.
    ///
    /// Unlike [`WalReader::replay_after`], this method is corruption-tolerant:
    /// - An incomplete last line (no trailing newline) is silently skipped.
    /// - Lines whose JSON cannot be parsed are skipped with a warning.
    /// - Lines whose sequence number cannot be parsed are skipped with a warning.
    ///
    /// This makes it safe to call after a crash where the last write may have been
    /// only partially flushed to disk.
    pub fn replay_from(path: impl AsRef<Path>, after_seq: u64) -> io::Result<Vec<(u64, Command)>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut line_buf = String::new();

        // Determine whether the file ends with a newline so we can detect a
        // partial trailing line left by a crash.
        let file_ends_with_newline = {
            let mut probe = File::open(path)?;
            let file_len = probe.seek(SeekFrom::End(0))?;
            if file_len == 0 {
                true // Treat empty file as clean.
            } else {
                probe.seek(SeekFrom::End(-1))?;
                let mut last = [0u8; 1];
                probe.read_exact(&mut last)?;
                last[0] == b'\n'
            }
        };

        // Count total lines so we can identify the last one.
        let total_lines = {
            let mut count = 0u64;
            let mut tmp_reader = BufReader::new(File::open(path)?);
            let mut tmp_buf = String::new();
            while tmp_reader.read_line(&mut tmp_buf).unwrap_or(0) > 0 {
                if !tmp_buf.trim().is_empty() {
                    count += 1;
                }
                tmp_buf.clear();
            }
            count
        };

        let mut current_line = 0u64;

        loop {
            line_buf.clear();
            let bytes_read = reader.read_line(&mut line_buf)?;
            if bytes_read == 0 {
                break; // EOF
            }

            let trimmed = line_buf.trim_end();
            if trimmed.is_empty() {
                continue;
            }

            current_line += 1;

            // If this is the last non-empty line and the file doesn't end with
            // a newline, treat it as a partial write and skip with a warning.
            if current_line == total_lines && !file_ends_with_newline {
                tracing::warn!(
                    path = %path.display(),
                    "WAL replay: skipping incomplete trailing line (no newline — likely a crash mid-write)"
                );
                continue;
            }

            // Parse: "seq\tjson"
            let (seq_str, json_str) = match trimmed.split_once('\t') {
                Some(parts) => parts,
                None => {
                    tracing::warn!(
                        path = %path.display(),
                        line = current_line,
                        "WAL replay: skipping malformed line (no tab separator)"
                    );
                    continue;
                }
            };

            let seq: u64 = match seq_str.parse() {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        line = current_line,
                        error = %e,
                        "WAL replay: skipping line with unparseable sequence number"
                    );
                    continue;
                }
            };

            if seq <= after_seq {
                continue;
            }

            let cmd: Command = match serde_json::from_str(json_str) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        seq = seq,
                        error = %e,
                        "WAL replay: skipping seq {} — JSON parse error"
                    , seq);
                    continue;
                }
            };

            entries.push((seq, cmd));
        }

        Ok(entries)
    }

    /// Read the last committed sequence number from a WAL file without loading
    /// all entries into memory.
    ///
    /// Strategy: seek to the end of the file and scan backwards for the last
    /// complete line, then parse its sequence prefix.
    ///
    /// Returns `0` if the file does not exist or contains no complete entries.
    pub fn last_sequence(path: impl AsRef<Path>) -> io::Result<u64> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(0);
        }

        let mut file = File::open(path)?;
        let file_len = file.seek(SeekFrom::End(0))?;
        if file_len == 0 {
            return Ok(0);
        }

        // Read up to the last 4 KiB — enough for any single WAL line.
        const SCAN_WINDOW: u64 = 4096;
        let scan_start = file_len.saturating_sub(SCAN_WINDOW);
        file.seek(SeekFrom::Start(scan_start))?;

        let mut tail = Vec::new();
        file.read_to_end(&mut tail)?;

        // Find the second-to-last newline (the one that terminates the last *complete* line).
        // Positions are relative to `tail`.
        let last_newline = tail.iter().rposition(|&b| b == b'\n');

        let search_end = match last_newline {
            // The file ends with '\n' — the last complete line ends here.
            // We want the newline *before* that, i.e., the end of the previous line.
            Some(pos) if pos + 1 == tail.len() => {
                // File ends with '\n'. The last complete line is everything before it.
                // Find the preceding newline to isolate that line.
                tail[..pos].iter().rposition(|&b| b == b'\n').map(|p| p + 1).unwrap_or(0)
            }
            // File does NOT end with '\n' — last "line" is a partial write.
            // The last *complete* line ends at `last_newline`.
            Some(pos) => {
                tail[..pos].iter().rposition(|&b| b == b'\n').map(|p| p + 1).unwrap_or(0)
            }
            // No newline at all — there are no complete lines.
            None => return Ok(0),
        };

        // Find the newline that terminates the last complete line.
        let line_end = match last_newline {
            Some(pos) if pos + 1 == tail.len() => pos, // ends with '\n'
            Some(pos) => pos,                           // partial trailing line
            None => return Ok(0),
        };

        let line_bytes = &tail[search_end..line_end];
        let line = std::str::from_utf8(line_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
            .trim();

        if line.is_empty() {
            return Ok(0);
        }

        let seq_str = line.split('\t').next().unwrap_or("");
        seq_str.parse::<u64>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("WAL last_sequence: cannot parse '{}': {}", seq_str, e),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_cmd(side: Side, price: rust_decimal::Decimal, qty: rust_decimal::Decimal) -> Command {
        Command::PlaceOrder(PlaceOrderCommand {
            order_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price,
            qty,
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

    #[test]
    fn test_append_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        // Write 3 commands
        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            assert_eq!(wal.sequence(), 0);

            let seq1 = wal.append(&make_cmd(Side::Buy, dec!(50000), dec!(1))).unwrap();
            assert_eq!(seq1, 1);

            let seq2 = wal.append(&make_cmd(Side::Sell, dec!(51000), dec!(0.5))).unwrap();
            assert_eq!(seq2, 2);

            let seq3 = wal.append(&Command::CancelOrder(CancelOrderCommand {
                order_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                symbol: Symbol::new("BTCUSDT"),
                timestamp: chrono::Utc::now(),
            })).unwrap();
            assert_eq!(seq3, 3);
        }

        // Read back
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.map(|r| r.unwrap()).collect();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, 1);
        assert_eq!(entries[1].0, 2);
        assert_eq!(entries[2].0, 3);

        // Verify command types
        assert!(matches!(entries[0].1, Command::PlaceOrder(_)));
        assert!(matches!(entries[1].1, Command::PlaceOrder(_)));
        assert!(matches!(entries[2].1, Command::CancelOrder(_)));
    }

    #[test]
    fn test_resume_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        // Write 2 commands, close
        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            wal.append(&make_cmd(Side::Buy, dec!(50000), dec!(1))).unwrap();
            wal.append(&make_cmd(Side::Sell, dec!(51000), dec!(0.5))).unwrap();
        }

        // Reopen — should resume from seq 2
        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            assert_eq!(wal.sequence(), 2);

            let seq3 = wal.append(&make_cmd(Side::Buy, dec!(49000), dec!(2))).unwrap();
            assert_eq!(seq3, 3);
        }

        // Read all — should have 3 entries
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.map(|r| r.unwrap()).collect();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_deterministic_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        // Create commands with fixed UUIDs for deterministic comparison
        let order_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        let cmd = Command::PlaceOrder(PlaceOrderCommand {
            order_id,
            user_id,
            symbol: Symbol::new("ETHUSDT"),
            side: Side::Buy,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Ioc,
            price: dec!(0),
            qty: dec!(10),
            timestamp: chrono::DateTime::from_timestamp(1700000000, 0).unwrap().to_utc(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        });

        // Write
        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            wal.append(&cmd).unwrap();
        }

        // Read back and verify exact match
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.map(|r| r.unwrap()).collect();

        if let Command::PlaceOrder(ref replayed) = entries[0].1 {
            assert_eq!(replayed.order_id, order_id);
            assert_eq!(replayed.user_id, user_id);
            assert_eq!(replayed.symbol, Symbol::new("ETHUSDT"));
            assert_eq!(replayed.side, Side::Buy);
            assert_eq!(replayed.order_type, OrderType::Market);
            assert_eq!(replayed.time_in_force, TimeInForce::Ioc);
            assert_eq!(replayed.qty, dec!(10));
        } else {
            panic!("Expected PlaceOrder");
        }
    }

    #[test]
    fn test_replay_into_matching_engine() {
        use exchange_matching_engine::MatchingEngine;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        let user = Uuid::new_v4();

        // Write commands to WAL
        let cmds = vec![
            Command::PlaceOrder(PlaceOrderCommand {
                order_id: Uuid::new_v4(),
                user_id: user,
                symbol: Symbol::new("BTCUSDT"),
                side: Side::Sell,
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
            }),
            Command::PlaceOrder(PlaceOrderCommand {
                order_id: Uuid::new_v4(),
                user_id: user,
                symbol: Symbol::new("BTCUSDT"),
                side: Side::Sell,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: dec!(50100),
                qty: dec!(2),
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
            }),
        ];

        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            for cmd in &cmds {
                wal.append(cmd).unwrap();
            }
        }

        // Replay into a fresh matching engine
        let mut engine = MatchingEngine::new();
        engine.add_symbol(Symbol::new("BTCUSDT"));

        let reader = WalReader::open(&path).unwrap();
        for entry in reader {
            let (_, cmd) = entry.unwrap();
            engine.process(cmd);
        }

        // Verify engine state matches what we expect
        let book = engine.book(&Symbol::new("BTCUSDT")).unwrap();
        assert_eq!(book.order_count(), 2);
        assert_eq!(book.best_ask().unwrap().0, dec!(50000));
    }

    #[test]
    fn test_empty_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wal");

        let wal = WriteAheadLog::open(&path, false).unwrap();
        assert_eq!(wal.sequence(), 0);

        // Read empty
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.map(|r| r.unwrap()).collect();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_truncate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trunc.wal");

        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            wal.append(&make_cmd(Side::Buy, dec!(50000), dec!(1))).unwrap();
            wal.append(&make_cmd(Side::Sell, dec!(51000), dec!(2))).unwrap();
            assert_eq!(wal.sequence(), 2);

            wal.truncate().unwrap();
            assert_eq!(wal.sequence(), 0);
        }

        // File should be empty after truncate
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.map(|r| r.unwrap()).collect();
        assert_eq!(entries.len(), 0);
    }

    // Test 5: corrupted trailing line (no newline at end) is skipped gracefully.
    #[test]
    fn test_replay_from_corrupted_trailing_line() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.wal");

        // Write two valid entries.
        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            wal.append(&make_cmd(Side::Buy, dec!(50000), dec!(1))).unwrap();
            wal.append(&make_cmd(Side::Sell, dec!(51000), dec!(1))).unwrap();
        }

        // Simulate a crash mid-write: append a partial line (no newline).
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            // Partial JSON — no newline, as if flushed mid-write during a crash.
            file.write_all(b"3\t{\"PlaceOrder\":{\"order_id\":\"incomplete").unwrap();
        }

        // replay_from must return only the 2 complete entries, not error.
        let entries = WriteAheadLog::replay_from(&path, 0).unwrap();
        assert_eq!(
            entries.len(),
            2,
            "expected 2 complete entries, got {}",
            entries.len()
        );
        assert_eq!(entries[0].0, 1);
        assert_eq!(entries[1].0, 2);
    }

    // Test: replay_from with after_seq filters correctly.
    #[test]
    fn test_replay_from_after_seq() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            for _ in 0..5 {
                wal.append(&make_cmd(Side::Buy, dec!(50000), dec!(1))).unwrap();
            }
        }

        // Request entries after seq=3 → should get seqs 4 and 5.
        let entries = WriteAheadLog::replay_from(&path, 3).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, 4);
        assert_eq!(entries[1].0, 5);
    }

    // Test: last_sequence returns the highest committed seq without loading all entries.
    #[test]
    fn test_last_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        // Empty file → 0.
        {
            WriteAheadLog::open(&path, false).unwrap();
        }
        assert_eq!(WriteAheadLog::last_sequence(&path).unwrap(), 0);

        // Write 3 entries.
        {
            let mut wal = WriteAheadLog::open(&path, false).unwrap();
            wal.append(&make_cmd(Side::Buy, dec!(50000), dec!(1))).unwrap();
            wal.append(&make_cmd(Side::Sell, dec!(51000), dec!(1))).unwrap();
            wal.append(&make_cmd(Side::Buy, dec!(49000), dec!(2))).unwrap();
        }
        assert_eq!(WriteAheadLog::last_sequence(&path).unwrap(), 3);
    }

    // Test: last_sequence on non-existent file returns 0.
    #[test]
    fn test_last_sequence_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_such.wal");
        assert_eq!(WriteAheadLog::last_sequence(&path).unwrap(), 0);
    }
}

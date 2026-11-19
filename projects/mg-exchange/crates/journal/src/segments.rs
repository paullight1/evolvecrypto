use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use exchange_common::Command;

use crate::wal::WriteAheadLog;

/// Default maximum number of entries before rotating to a new segment.
const DEFAULT_MAX_ENTRIES_PER_SEGMENT: u64 = 1_000_000;

/// Default number of segment files to retain (oldest are deleted beyond this limit).
const DEFAULT_MAX_SEGMENTS: usize = 3;

/// Segment file naming prefix and extension.
const SEGMENT_PREFIX: &str = "commands_";
const SEGMENT_EXT: &str = ".wal";

/// A WAL manager that rotates into new segment files when the current one
/// exceeds `max_entries_per_segment` entries, and prunes the oldest segments
/// when more than `max_segments` files exist.
///
/// Segment files are named `commands_000001.wal`, `commands_000002.wal`, etc.
/// Lexicographic ordering of file names equals chronological ordering because
/// IDs are zero-padded to six digits.
pub struct SegmentedWal {
    /// Directory that holds all segment files.
    dir: PathBuf,
    /// Currently open segment, accepting new writes.
    current: WriteAheadLog,
    /// Number of entries written to the current segment so far.
    current_entries: u64,
    /// Rotate when this many entries have been written to the current segment.
    max_entries_per_segment: u64,
    /// Delete oldest segments when the total count exceeds this value.
    max_segments: usize,
    /// Monotonically increasing segment identifier (appears in the file name).
    current_segment_id: u64,
    /// Whether to fsync after each write (mirrors the inner WAL flag).
    durable: bool,
}

impl SegmentedWal {
    /// Open (or create) a segmented WAL in `dir` with production defaults.
    ///
    /// Enables fsync for durability. Use [`Self::open_with_limits`] with
    /// `durable = false` for tests and benchmarks.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_limits(dir, DEFAULT_MAX_ENTRIES_PER_SEGMENT, DEFAULT_MAX_SEGMENTS, true)
    }

    /// Open with explicit rotation / retention limits and durability choice.
    pub fn open_with_limits(
        dir: impl AsRef<Path>,
        max_entries_per_segment: u64,
        max_segments: usize,
        durable: bool,
    ) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        let existing = list_segments(&dir)?;

        let (current_segment_id, current_entries) = if let Some(last_path) = existing.last() {
            let id = segment_id_from_path(last_path)?;
            let entries = count_entries(last_path)?;
            (id, entries)
        } else {
            // No segments yet — first segment will have ID 1.
            (1, 0)
        };

        let seg_path = segment_path(&dir, current_segment_id);
        let current = WriteAheadLog::open(&seg_path, durable)?;

        Ok(Self {
            dir,
            current,
            current_entries,
            max_entries_per_segment,
            max_segments,
            current_segment_id,
            durable,
        })
    }

    /// Append a command to the current segment.
    ///
    /// Rotates to a new segment first if the current one is full.
    /// Returns the WAL sequence number of the appended entry.
    pub fn append(&mut self, command: &Command) -> io::Result<u64> {
        if self.current_entries >= self.max_entries_per_segment {
            self.rotate()?;
        }

        let seq = self.current.append(command)?;
        self.current_entries += 1;
        Ok(seq)
    }

    /// Append multiple commands with a single fsync at the end (group commit).
    ///
    /// Each command is serialized and flushed to the OS page cache individually,
    /// but fdatasync is called only once after all writes complete. This gives
    /// the same durability guarantee as per-entry fsync but with ~100× fewer
    /// syscalls under load.
    ///
    /// Returns a Vec of sequence numbers, one per command.
    pub fn append_batch(&mut self, commands: &[Command]) -> io::Result<Vec<u64>> {
        let mut seqs = Vec::with_capacity(commands.len());

        for cmd in commands {
            if self.current_entries >= self.max_entries_per_segment {
                self.rotate()?;
            }
            let seq = self.current.append_no_sync(cmd)?;
            self.current_entries += 1;
            seqs.push(seq);
        }

        // Single fsync covers all entries in the batch.
        if self.durable && !commands.is_empty() {
            self.current.sync()?;
        }

        Ok(seqs)
    }

    /// Close the current segment and start a new one.
    ///
    /// The new segment's sequence counter is seeded from the last sequence
    /// number in the closing segment so that global sequence numbers remain
    /// strictly monotonically increasing across segment boundaries.
    ///
    /// Deletes the oldest segment file when the total count would exceed
    /// `max_segments`.
    pub fn rotate(&mut self) -> io::Result<()> {
        // Capture the last global sequence before dropping the current segment.
        let last_seq = self.current.sequence();

        self.current_segment_id += 1;
        let new_path = segment_path(&self.dir, self.current_segment_id);

        tracing::info!(
            segment_id = self.current_segment_id,
            last_seq,
            path = %new_path.display(),
            "WAL segment rotated"
        );

        // Dropping the old WriteAheadLog flushes and closes the file.
        // Seed the new WAL's counter so sequences continue from where we left off.
        self.current = WriteAheadLog::open_with_starting_seq(&new_path, self.durable, last_seq)?;
        self.current_entries = 0;

        self.prune_old_segments()?;
        Ok(())
    }

    /// Replay all commands across all segments in chronological order.
    ///
    /// Uses the corruption-tolerant [`WriteAheadLog::replay_from`] on each
    /// segment so a partial trailing write (from a crash) is handled gracefully.
    pub fn replay_all(&self) -> io::Result<Vec<(u64, Command)>> {
        self.replay_after(0)
    }

    /// Replay all commands with sequence number strictly greater than `after_seq`.
    pub fn replay_after(&self, after_seq: u64) -> io::Result<Vec<(u64, Command)>> {
        let segments = list_segments(&self.dir)?;
        let mut result = Vec::new();

        for seg_path in &segments {
            let entries = WriteAheadLog::replay_from(seg_path, after_seq)?;
            result.extend(entries);
        }

        Ok(result)
    }

    /// Flush the current segment's BufWriter to disk.
    ///
    /// The WAL already calls `flush` after every `append`, so this is mainly
    /// used during graceful shutdown to ensure no data lingers in userspace
    /// buffers.
    pub fn flush(&mut self) -> io::Result<()> {
        self.current.flush()
    }

    /// Current WAL sequence number (last written entry's seq across all segments).
    pub fn sequence(&self) -> u64 {
        self.current.sequence()
    }

    /// Directory that holds all segment files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Current segment ID.
    pub fn current_segment_id(&self) -> u64 {
        self.current_segment_id
    }

    // --- Private helpers ---

    fn prune_old_segments(&self) -> io::Result<()> {
        let mut segments = list_segments(&self.dir)?;

        while segments.len() > self.max_segments {
            let oldest = segments.remove(0);
            tracing::info!(
                path = %oldest.display(),
                max_segments = self.max_segments,
                "WAL segment pruned"
            );
            fs::remove_file(&oldest)?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// File-system helpers
// ---------------------------------------------------------------------------

/// Build the path for a segment with the given ID.
fn segment_path(dir: &Path, id: u64) -> PathBuf {
    dir.join(format!("{}{:06}{}", SEGMENT_PREFIX, id, SEGMENT_EXT))
}

/// Extract the numeric segment ID from a path like `.../commands_000003.wal`.
fn segment_id_from_path(path: &Path) -> io::Result<u64> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 segment file name"))?;

    let digits = stem.strip_prefix(SEGMENT_PREFIX).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected segment name '{}' (missing prefix '{}')", stem, SEGMENT_PREFIX),
        )
    })?;

    digits.parse::<u64>().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cannot parse segment id from '{}': {}", digits, e),
        )
    })
}

/// List all `commands_NNNNNN.wal` files in `dir`, sorted lexicographically
/// (which equals chronological order given the zero-padded names).
pub(crate) fn list_segments(dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if name.starts_with(SEGMENT_PREFIX) && name.ends_with(SEGMENT_EXT) {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    paths.sort();
    Ok(paths)
}

/// Count the number of non-empty lines (entries) in a WAL segment file.
fn count_entries(path: &Path) -> io::Result<u64> {
    use std::io::BufRead;
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let count = reader
        .lines()
        .filter(|l| l.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false))
        .count() as u64;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_place(price: rust_decimal::Decimal) -> Command {
        Command::PlaceOrder(PlaceOrderCommand {
            order_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price,
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

    // Test 6: segment rotation creates multiple files when max_entries is exceeded.
    #[test]
    fn test_segment_rotation() {
        let dir = tempfile::tempdir().unwrap();

        {
            // max_entries=3 → writing 10 entries triggers multiple rotations.
            let mut wal = SegmentedWal::open_with_limits(dir.path(), 3, 10, false).unwrap();
            for i in 0u32..10 {
                wal.append(&make_place(dec!(50000) + rust_decimal::Decimal::from(i))).unwrap();
            }
        }

        let segments = list_segments(dir.path()).unwrap();
        // 10 entries at max 3 per segment → 4 segments (3+3+3+1).
        assert!(
            segments.len() >= 2,
            "expected multiple segments, got {}",
            segments.len()
        );
    }

    // Test 7: replay across all segments produces entries in correct chronological order.
    #[test]
    fn test_segment_replay_order() {
        let dir = tempfile::tempdir().unwrap();

        {
            let mut wal = SegmentedWal::open_with_limits(dir.path(), 3, 10, false).unwrap();
            for i in 1u32..=9 {
                wal.append(&make_place(rust_decimal::Decimal::from(i))).unwrap();
            }
        }

        let wal = SegmentedWal::open_with_limits(dir.path(), 3, 10, false).unwrap();
        let entries = wal.replay_all().unwrap();

        assert_eq!(entries.len(), 9);

        // Sequence numbers must be strictly monotonically increasing.
        let seqs: Vec<u64> = entries.iter().map(|(s, _)| *s).collect();
        for window in seqs.windows(2) {
            assert!(
                window[1] > window[0],
                "sequences not monotonically increasing: {:?}",
                seqs
            );
        }
    }

    // Test 8: old segments are deleted when max_segments is exceeded.
    #[test]
    fn test_old_segments_pruned() {
        let dir = tempfile::tempdir().unwrap();

        {
            // max_segments=2, max_entries=2 → only 2 segment files retained.
            let mut wal = SegmentedWal::open_with_limits(dir.path(), 2, 2, false).unwrap();
            for i in 0u32..10 {
                wal.append(&make_place(dec!(100) + rust_decimal::Decimal::from(i))).unwrap();
            }
        }

        let segments = list_segments(dir.path()).unwrap();
        assert!(
            segments.len() <= 2,
            "expected at most 2 segments after pruning, got {}",
            segments.len()
        );
    }

    // Test 10: replay_after returns only entries with seq > after_seq.
    #[test]
    fn test_replay_after_seq() {
        let dir = tempfile::tempdir().unwrap();

        let mut wal = SegmentedWal::open_with_limits(dir.path(), 10, 5, false).unwrap();
        for i in 0u32..5 {
            wal.append(&make_place(dec!(1000) + rust_decimal::Decimal::from(i))).unwrap();
        }

        // Entries have seqs 1..=5. Request entries after seq=3 → seqs 4 and 5.
        let entries = wal.replay_after(3).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, 4);
        assert_eq!(entries[1].0, 5);
    }
}

//! Engine state snapshot: write and load full-fidelity order book state.
//!
//! The snapshot file contains every resting order across all symbols, plus
//! a WAL sequence marker. On startup, loading the snapshot and replaying
//! the WAL only from the snapshot sequence cuts recovery time from O(full
//! history) to O(recent commands).

use std::io;
use std::path::Path;

use exchange_matching_engine::FullBookSnapshot;
use serde::{Deserialize, Serialize};

/// On-disk format for the engine state snapshot.
#[derive(Debug, Serialize, Deserialize)]
pub struct EngineStateSnapshot {
    /// The WAL sequence number at the time the snapshot was taken.
    /// On replay, skip all WAL entries with seq <= this value.
    pub wal_sequence: u64,
    /// One FullBookSnapshot per symbol.
    pub books: Vec<FullBookSnapshot>,
    /// ISO-8601 timestamp when the snapshot was written.
    pub created_at: String,
}

/// Write an engine snapshot to a JSON file.
///
/// Atomically: writes to a `.tmp` file first, then renames. This prevents
/// a crash during write from corrupting the snapshot.
pub fn write_snapshot(path: &Path, snapshot: &EngineStateSnapshot) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string(snapshot)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Load an engine snapshot from a JSON file. Returns `None` if the file
/// doesn't exist.
pub fn load_snapshot(path: &Path) -> io::Result<Option<EngineStateSnapshot>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)?;
    let snap: EngineStateSnapshot = serde_json::from_str(&data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(snap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snap.json");

        let snap = EngineStateSnapshot {
            wal_sequence: 42,
            books: vec![],
            created_at: "2026-04-16T12:00:00Z".to_string(),
        };

        write_snapshot(&path, &snap).unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded.wal_sequence, 42);
        assert!(loaded.books.is_empty());
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        assert!(load_snapshot(&path).unwrap().is_none());
    }
}

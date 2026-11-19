use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use exchange_common::Command;

/// Reader for replaying WAL entries.
///
/// Iterates over entries in order, yielding (sequence_number, command) pairs.
/// Used for crash recovery: replay all entries into a fresh matching engine.
pub struct WalReader {
    reader: BufReader<File>,
    line_buf: String,
}

impl WalReader {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
            line_buf: String::new(),
        })
    }

    /// Replay all commands from a WAL file from a given sequence number.
    /// Returns commands with seq > `after_seq`.
    pub fn replay_after(path: impl AsRef<Path>, after_seq: u64) -> io::Result<Vec<(u64, Command)>> {
        let reader = Self::open(path)?;
        let mut entries = Vec::new();
        for result in reader {
            let (seq, cmd) = result?;
            if seq > after_seq {
                entries.push((seq, cmd));
            }
        }
        Ok(entries)
    }
}

impl Iterator for WalReader {
    type Item = io::Result<(u64, Command)>;

    fn next(&mut self) -> Option<Self::Item> {
        self.line_buf.clear();
        match self.reader.read_line(&mut self.line_buf) {
            Ok(0) => None, // EOF
            Ok(_) => {
                let line = self.line_buf.trim_end();
                if line.is_empty() {
                    return self.next(); // Skip empty lines
                }

                // Parse: "seq\tjson"
                let (seq_str, json_str) = match line.split_once('\t') {
                    Some(parts) => parts,
                    None => {
                        return Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Malformed WAL line (no tab): {}", line),
                        )));
                    }
                };

                let seq: u64 = match seq_str.parse() {
                    Ok(s) => s,
                    Err(e) => {
                        return Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Invalid sequence number '{}': {}", seq_str, e),
                        )));
                    }
                };

                let cmd: Command = match serde_json::from_str(json_str) {
                    Ok(c) => c,
                    Err(e) => {
                        return Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Invalid command JSON at seq {}: {}", seq, e),
                        )));
                    }
                };

                Some(Ok((seq, cmd)))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

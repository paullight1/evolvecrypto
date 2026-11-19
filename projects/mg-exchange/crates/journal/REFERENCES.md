# Journal (WAL) — Business Logic References

Traces write-ahead log and crash recovery to industry standards.

## 30. WAL (Write-Ahead Log) -- Crash Recovery

### Mechanism

Every command is durably persisted to an append-only log BEFORE entering the matching
engine. On crash recovery, the WAL is replayed to reconstruct the matching engine state.

### A. Mathematical Derivation

**WAL guarantees:**

1. **Durability:** Each entry is fsync'd before the command is processed.
   `P(data_loss | crash_after_fsync) = 0` (assuming non-corrupt storage).

2. **Ordering:** Sequence numbers are monotonically increasing and gap-free.
   `seq_{n+1} = seq_n + 1` for all entries.

3. **Idempotent replay:** Replaying the same command twice produces the same state.
   For `PlaceOrder`: if the order already exists, it's a no-op.
   For `CancelOrder`: if the order is already cancelled, it's a no-op.

**Segment rotation:**
```
if current_segment_entries >= max_entries_per_segment:
    create new segment file
    current_segment = commands_{next_id:06d}.wal
    current_entries = 0
```

**Retention:**
```
if count(segment_files) > max_segments:
    delete oldest segment files until count <= max_segments
```

Default: 1,000,000 entries per segment, retain 3 segments.

**Recovery flow:**
```
1. Read all WAL entries from all segments (ordered by sequence)
2. For each entry (seq, command):
   a. If command references a terminal order (filled/cancelled in DB): SKIP
   b. Otherwise: replay into matching engine
3. Matching engine state is now identical to pre-crash state
```

### B. Worked Numerical Example

**WAL file format:**
```
1\t{"PlaceOrder":{"order_id":"abc-123","user_id":"00000000-0000-0000-0000-000000000001","symbol":"BTCUSDT","side":"Buy","price":"65000","qty":"1","order_type":"Limit","time_in_force":"GTC"}}
2\t{"PlaceOrder":{"order_id":"def-456","user_id":"00000000-0000-0000-0000-000000000002","symbol":"BTCUSDT","side":"Sell","price":"65000","qty":"0.5","order_type":"Limit","time_in_force":"GTC"}}
3\t{"CancelOrder":{"order_id":"abc-123"}}
```

**Crash at sequence 2 (after write, before matching engine processes):**

On restart:
1. WAL reader reads entries: [(1, PlaceOrder abc-123), (2, PlaceOrder def-456)]
2. Entry 3 was never written (crash happened before cancel was committed).
3. Replay entry 1: Place buy order abc-123 at $65,000, 1 BTC.
4. Replay entry 2: Place sell order def-456 at $65,000, 0.5 BTC.
5. Matching engine matches: 0.5 BTC fill at $65,000. abc-123 has 0.5 BTC remaining.

State is now identical to what it would have been if no crash occurred.

**Segment rotation example:**
- Segment 1: `commands_000001.wal` (entries 1--1,000,000)
- Segment 2: `commands_000002.wal` (entries 1,000,001--2,000,000)
- Segment 3: `commands_000003.wal` (entries 2,000,001--3,000,000)
- Segment 4 starts: `commands_000004.wal` (entries 3,000,001+)

With max_segments=3, segment 1 is now deleted. Entries 1--1,000,000 are lost.
This is safe only if a matching engine snapshot covers up to entry 1,000,000.

### C. Reference Implementation Comparison

| System | WAL Design | Difference |
|--------|-----------|------------|
| **ARIES** (Mohan et al., 1992. "ARIES: A Transaction Recovery Method." *ACM TODS*) | Write-ahead logging with redo/undo recovery. Page-level logging. Fuzzy checkpointing. | Our WAL is command-level (not page-level). We only need redo (no undo, because matching is deterministic). Simpler than ARIES. |
| **PostgreSQL WAL** (https://www.postgresql.org/docs/current/wal-intro.html) | Write-ahead logging for crash recovery. Segment files (16MB default). Continuous archiving. | Similar segment rotation concept. PostgreSQL has LSN (Log Sequence Number) analogous to our sequence number. |
| **Apache Kafka** | Append-only commit log with segment rotation and retention. | Kafka's partition log is conceptually identical to our WAL segments. We are single-node; Kafka is distributed. |
| **LMAX Disruptor** | Event journal for deterministic replay. Single-writer pattern. | Same philosophy: log events before processing. LMAX uses memory-mapped files for lower latency. |

### D. Edge Cases & Boundary Behavior

- **Empty WAL on first start:** Sequence starts at 0. First entry gets sequence 1.
- **Corrupt entry:** WAL reader skips malformed lines. Logged as error in `ReplayResult::errors`.
- **Gap in sequence numbers:** The replay is gap-tolerant. Commands are processed regardless of gaps.
- **Balance events during replay:** `BalanceDeposit`, `BalanceWithdraw`, `FundingPayment` are skipped by the matching engine replay (they are for the account layer, not the order book).
- **Terminal orders during replay:** If an order is already filled/cancelled (known from DB), the WAL entry is skipped. This prevents re-matching already-settled trades.
- **Durable = false:** For tests and benchmarks, fsync is skipped. Not safe for production.
- **Concurrent writes:** Not supported. Single-writer design consistent with single-threaded matching.

### E. Test Coverage Reference

- **File:** `crates/journal/src/wal.rs` (inline tests)
- **File:** `crates/journal/src/segments.rs` (inline tests)
- **File:** `crates/journal/src/replay.rs` (inline tests)
- **Key tests:** `test_append_and_read`, `test_sequence_continuity`, `test_segment_rotation`, `test_replay_skip_terminal`, `test_replay_skip_balance_events`, `test_truncate`
- **Test values:** 100--10,000 entries, segment rotation at 100 entries.

### Our Implementation

- **Code:** `crates/journal/src/wal.rs` (WriteAheadLog: append, fsync, truncate)
- **Code:** `crates/journal/src/segments.rs` (SegmentedWal: rotation, retention)
- **Code:** `crates/journal/src/replay.rs` (replay_into_engine: idempotent recovery)
- **Code:** `crates/journal/src/reader.rs` (WalReader: sequential read)
- Format: `{seq}\t{json}\n` per entry.
- fsync after each write (configurable).
- Segment rotation: 1M entries/segment, retain 3 segments.
- Replay: idempotent, gap-tolerant, skips terminal orders and balance events.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Mohan, C. et al. (1992). "ARIES: A Transaction Recovery Method Supporting Fine-Granularity Locking and Partial Rollbacks Using Write-Ahead Logging." *ACM Transactions on Database Systems*, 17(1), 94-162. | The foundational WAL recovery protocol. Our command-level WAL is a simplification of ARIES (redo-only, no undo). |
| PostgreSQL. "Write-Ahead Logging (WAL)." (https://www.postgresql.org/docs/current/wal-intro.html) | Production WAL implementation with segment rotation. |
| LMAX Exchange (2011). "The LMAX Architecture." (Martin Fowler) | Event sourcing with deterministic replay for exchange matching engines. |
| Kleppmann, M. (2017). *Designing Data-Intensive Applications*. O'Reilly. Chapter 11: Stream Processing. | Comprehensive treatment of event logs, compaction, and replay semantics. |

### Compliance

**Match.** Our WAL follows the ARIES principle (write before state mutation) adapted for
a deterministic matching engine. Segment rotation with retention matches PostgreSQL and
Kafka patterns. Idempotent replay ensures crash recovery produces identical state.

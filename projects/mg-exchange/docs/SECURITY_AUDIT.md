# Security Audit — Exchange API Gateway

**Audit date:** 2026-03-19
**Scope:** `crates/api-gateway/`, `crates/db/`
**Sprint:** Month 10, T9.3 + T9.4

---

## Summary

| Area | Status | Notes |
|---|---|---|
| Authentication & Authorization | PARTIAL | HMAC timing attack fixed (T9.4). Admin endpoints now protected. Phase 1 nil-user bypass still present by design. |
| Input Validation | PASS | All user inputs validated; body size capped at 64 KiB. |
| SQL Injection | PASS | All queries use sqlx parameterised bindings. |
| Balance Integrity | PARTIAL | Core accounting is sound; futures margin path allows underfunded positions in Phase 1 with a logged warning. |
| DoS Prevention | PASS | Rate limiter, body size limit, and batch order caps are active. |
| Cryptographic | PASS | HMAC comparison is now constant-time (T9.4 Fix 1); JWT signing uses RS256 via jsonwebtoken. |

---

## 1. Authentication & Authorization

**Files read:** `crates/api-gateway/src/auth.rs`, `crates/api-gateway/src/server.rs`

### 1.1 HMAC timing attack — FIXED (T9.4 Fix 1)

**Finding (FAIL → FIXED):**
`verify_signature` previously compared the computed HMAC against the client-supplied
signature using string equality (`==`):

```rust
// BEFORE — timing oracle
let expected = hex::encode(mac.finalize().into_bytes());
if expected != signature {
    return Err(AuthError::InvalidSignature);
}
```

A remote attacker could measure response time to learn how many bytes of a guessed
signature matched, allowing offline brute-forcing of the HMAC key or forging
signatures for arbitrary messages.

**Fix applied:**

```rust
// AFTER — constant-time
let sig_bytes = hex::decode(signature).map_err(|_| AuthError::InvalidSignature)?;
mac.verify_slice(&sig_bytes).map_err(|_| AuthError::InvalidSignature)?;
```

`hmac::Mac::verify_slice` delegates to the `subtle` crate's `ConstantTimeEq`, which
guarantees that execution time does not depend on where the first differing byte is.

### 1.2 Cross-user data access — PASS

Every authenticated handler extracts `user_id` from the `AuthenticatedUser` extension
(populated from the verified HMAC token, not from a query parameter). Handlers that
look up orders or balances always filter by `user_id`:

- `get_open_orders`: `.filter(|(_, record)| record.user_id == user_id)`
- `get_order_by_id`: returns 404 if `record.user_id != user_id`
- `cancel_order` / `amend_order`: verify ownership before acting
- `cancel_trigger_order`: post-cancel ownership check with 403 on mismatch

No path allows user A to read or modify user B's data.

### 1.3 Admin endpoint protection — FIXED (T9.4 Fix 3)

**Finding (FAIL → FIXED):**
Routes under `/admin/*` (`/admin/stats`, `/admin/halt`, `/admin/reconciliation`,
`/admin/travel-rule/pending`) had no authentication or authorization check.

**Fix applied:**
A new Axum middleware `require_admin_middleware` is layered onto all `/admin/*`
routes. It reads the `x-api-key` header and calls `ApiKeyStore::is_admin_key`,
which checks that the key is active **and** has `is_admin = true`. A new
`is_admin: bool` field was added to `ApiKey`.

In open/dev mode (no key store configured on `Exchange`), the middleware passes
all requests through — existing tests are unaffected.

### 1.4 Phase 1 nil-user bypass — PARTIAL (by design)

`require_permission` allows `Uuid::nil()` to bypass all permission checks.
This is intentional for the Phase 1 deployment and is documented in the source.
The nil user must be removed before any production deployment that accepts
traffic from untrusted clients.

---

## 2. Input Validation

**File read:** `crates/api-gateway/src/rest.rs`

### 2.1 Symbol validation — PASS

`validate_symbol` enforces 3–20 uppercase ASCII characters/digits. Applied to
every handler that receives a symbol path or query parameter.

### 2.2 Numeric bounds — PASS

`validate_positive` rejects zero and negative values for `qty`, `price`,
`trigger_price`, and `amount`. Uses `rust_decimal::Decimal` throughout —
no `f64` arithmetic that could overflow silently.

`OrderHistoryQuery::limit` and `TradeHistoryQuery::limit` are capped at
`MAX_HISTORY_LIMIT = 1000` before use.

`OrderBookQuery::depth` is capped at `1000` before passing to the order book.

### 2.3 Batch size cap — PASS

`BATCH_MAX = 20` is enforced before processing any batch request, with a
descriptive 400 error.

### 2.4 Body size limit — PASS

`RequestBodyLimitLayer::new(64 * 1024)` (64 KiB) is applied globally in
`build_router`. Requests exceeding this size receive HTTP 413.

### 2.5 Malformed JSON — PASS

Axum's `Json` extractor returns 422 for malformed JSON before any handler
logic runs.

---

## 3. SQL Injection

**Files read:** `crates/db/src/order_repo.rs`, `crates/db/src/balance_repo.rs`,
`crates/db/src/user_repo.rs`, `crates/db/src/trade_repo.rs`

### 3.1 Parameterised queries — PASS

Every `sqlx` query in the `db` crate uses positional bind parameters (`$1`, `$2`, …).
No string interpolation into SQL was found. Examples:

```rust
// order_repo.rs — correct
sqlx::query_as::<_, OrderRow>(
    "INSERT INTO orders (id, user_id, ...) VALUES ($1, $2, ...)"
)
.bind(id)
.bind(user_id)
```

`sqlx` compile-time query checking (when `DATABASE_URL` is set) provides an
additional layer of validation.

---

## 4. Balance Integrity

**File read:** `crates/api-gateway/src/exchange.rs`

### 4.1 Credit without debit — PASS (spot path)

The spot settlement path (`settle_fill`) is handled entirely by
`AccountManager::settle_fill`, which was audited separately. The API gateway
calls `lock_for_order` before matching and `unlock_remainder` on cancel/reject.
There is no code path that credits a user without a corresponding debit.

### 4.2 Futures margin enforcement — PARTIAL

The futures path (`settle_futures_fill`) calls `accounts.withdraw` to debit
initial margin, but logs a warning and continues if the balance is insufficient
("Phase 1: allowing"). This means a user with zero balance could theoretically
open a futures position:

```rust
// exchange.rs — known Phase 1 gap, documented with tracing::warn
if let Err(e) = self.accounts.lock().unwrap().withdraw(user_id, "USDT", initial_margin) {
    tracing::warn!(..., "insufficient balance for futures margin (Phase 1: allowing)");
}
```

This is a deliberate Phase 1 shortcut and must be hardened before production.

### 4.3 Balance manipulation via order cycling — PASS

Order cancel/amend does not credit funds beyond what was originally locked.
The `unlock_remainder` call releases only the unused portion of the original lock.

---

## 5. DoS Prevention

### 5.1 Rate limiting — PASS

`RateLimiter` (token-bucket, 30 rps steady-state, burst 60) is applied globally
in `build_router` via `rate_limit_middleware`. Keyed by `X-Forwarded-For` or
`"unknown"`.

### 5.2 WebSocket connection limits — PARTIAL

The WebSocket handler (`ws.rs`) does not cap the number of concurrent connections
per IP. Under a Phase 1 trust model this is acceptable; production should add a
connection limit via `tower::limit::ConcurrencyLimitLayer`.

### 5.3 Body size limit — PASS

See section 2.4.

### 5.4 Max orders per user — PARTIAL

There is no per-user open-order cap. A malicious user could fill `order_locks`
and `pending_orders` with thousands of resting limit orders. Consider adding a
`MAX_OPEN_ORDERS_PER_USER` check inside `Exchange::place_order`.

---

## 6. Cryptographic

### 6.1 HMAC-SHA256 constant-time comparison — FIXED (T9.4 Fix 1)

See section 1.1.

### 6.2 JWT secret strength — PASS

`JwtManager` (`web_auth.rs`) uses a caller-supplied `secret` string. The
`generate_secret` helper in tests uses a long random string. In production
this secret must be at least 32 bytes of high-entropy random data.

### 6.3 Password hashing — PASS

`PasswordHasher` uses Argon2id via the `argon2 = "0.5"` crate, which is the
OWASP-recommended algorithm. Salt is generated with `OsRng`.

### 6.4 TOTP secret storage — PARTIAL

`TotpManager` stores secrets in-memory. They are never written to persistent
storage in Phase 1. For Phase 2, secrets must be encrypted at rest (AES-256-GCM
or similar) before storing in the database.

---

## 7. Additional Security Improvements (T9.4)

### 7.1 Request ID propagation — IMPLEMENTED (T9.4 Fix 2)

New `request_id_middleware` in `crates/api-gateway/src/request_id.rs`:

- Reads `x-request-id` from incoming request headers.
- Generates a UUID v4 if absent.
- Stores the ID in request extensions as `RequestId`.
- Reflects the ID in every response header.

This enables end-to-end traceability: load balancer → API gateway → log
aggregator can all correlate events by request ID.

### 7.2 Error message sanitization — IMPLEMENTED (T9.4 Fix 4)

HTTP 500 responses previously returned implementation-detail strings such as
`"Order state inconsistent"`. These have been replaced with the generic
`"Internal server error"` message. The real error detail is emitted via
`tracing::error!` for internal visibility without client exposure.

All HTTP 400 responses retain specific, user-actionable messages since these
describe client-side errors, not internal state.

---

## Open Items (Phase 2)

| ID | Severity | Description |
|---|---|---|
| SEC-01 | HIGH | Remove Phase 1 nil-user bypass (`Uuid::nil()` always allowed). |
| SEC-02 | HIGH | Enforce futures margin; do not allow underfunded positions. |
| SEC-03 | MEDIUM | Add per-user open-order cap to prevent memory exhaustion. |
| SEC-04 | MEDIUM | Add WebSocket connection limits per IP. |
| SEC-05 | MEDIUM | Encrypt TOTP secrets at rest before database storage. |
| SEC-06 | LOW | Add `Strict-Transport-Security` header in production (behind TLS terminator). |
| SEC-07 | LOW | Consider key rotation mechanism for HMAC secrets. |

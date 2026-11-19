# Custody — Production Hardening Roadmap

**Created:** 2026-03-24
**Status:** AUDIT DONE ✓ (Phase 2 external integration needed)
**Crate:** `crates/custody/`

---

## Audit Result

| Aspect | Rating | Notes |
|--------|--------|-------|
| CustodyProvider trait | A | Clean abstraction for Mock/Fireblocks |
| Deposit flow | B | Address gen + recording works, crediting is stub |
| Withdrawal flow | A- | Submit/approve/reject + daily limits + fees |
| Circuit breaker | A | $500K tx / $10M hourly / $50M daily + new-address spike |
| Address whitelist | A | 24h cooldown, soft-delete |
| Wallet management | A | Hot/warm/cold tiers + sweep logic |
| Fireblocks client | B | JWT signing + API stubs ready, feature-gated |
| Tests | A- | 68 tests |
| REFERENCES.md | A | BitGo/Fireblocks comparison, formulas |

## What's Code-Complete
- MockCustodyProvider (dev/test)
- DepositManager (address caching, record tracking)
- WithdrawalManager (tier-based daily limits, fee schedule, admin override)
- AddressWhitelist (24h cooldown)
- OutflowCircuitBreaker (3-level + new-address spike + auto-trip)
- WalletManager (hot/warm/cold with sweep)
- Fireblocks client stubs (JWT signing, asset mapping, webhook parsing)

## What Requires External Integration (Phase 2)
- [ ] Enable `--features fireblocks` in production build
- [ ] Fireblocks webhook receiver (POST /webhooks/fireblocks)
- [ ] Fireblocks webhook signature verification
- [ ] Background deposit crediting task (poll confirmations → credit)
- [ ] Auto-approve logic with AML gating
- [ ] Wire AddressWhitelist into withdrawal REST endpoint
- [ ] Price feed for real USD conversion in daily limits
- [ ] Test against Fireblocks sandbox

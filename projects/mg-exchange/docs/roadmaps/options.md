# Options — Production Hardening Roadmap

**Created:** 2026-03-24
**Status:** AUDIT DONE ✓ (prototype ~70%, needs product wiring)
**Crates:** `crates/risk-engine/` (pricing, margin, instruments, vol surface), `crates/settlement/` (options settlement)

---

## Audit Result

| Aspect | Rating | Notes |
|--------|--------|-------|
| Black-Scholes pricing | A | Correct, 27 tests, Hull reference values match |
| Greeks (delta/gamma/vega/theta/rho) | A | Tested ATM/OTM/ITM |
| IV inversion (Newton-Raphson) | A | Brenner-Subrahmanyam guess, 50 iter |
| Instrument generation | A | Strike grid, weekly/monthly/quarterly expiries |
| Vol surface | A- | Bilinear interp, fallback 80% IV |
| Short options margin | A | Bybit-style, 15 tests |
| Options settlement (expiry) | A- | ITM/worthless, cash-settled, 14 tests (now wired) |
| REST endpoints | B | 8 endpoints exist, synthetic fills |
| Tests | B+ | 70+ across all modules |

## What's Working
- Pricing + Greeks: production-grade
- Instrument chain: auto-generated with correct conventions
- Margin formulas: match Bybit
- Settlement logic: European cash-settlement
- Vol surface: ready for admin-loaded data

## What's Missing (Product-level)

### Needs Implementation
| # | Gap | Impact |
|---|-----|--------|
| 1 | No options order book (synthetic fills only) | Not a real trading product |
| 2 | Short margin not reserved from account on trade | Unlimited liability exposure |
| 3 | Vol surface not used in order pricing (hardcoded DEFAULT_IV) | Mark prices inaccurate |
| 4 | No expiry settlement background task | Options never settle |
| 5 | No cross-margin with perpetuals | Portfolio Greeks not used |
| 6 | In-memory positions only | Lost on restart |

### Already Wired (this session)
- [x] options_settlement.rs wired into settlement/lib.rs (was orphaned, +14 tests)
- [x] vol_surface wired into Exchange.get_option_mark_price() (Phase A)
- [x] Admin endpoints for vol surface loading (Phase D)

## Phase 2 Roadmap (when options become priority product)
1. Wire margin check into place_option_order (use options_margin::required_short_margin)
2. Add options expiry settlement background task (spawn_options_expiry_settler)
3. Replace synthetic fills with options order book (or RFQ model)
4. Integrate options positions into portfolio margin
5. Add WAL persistence for options positions

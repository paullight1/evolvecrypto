# Compliance — Production Hardening Roadmap

**Created:** 2026-03-24
**Status:** Sprint 1 DONE ✓ (Sprint 2 jurisdiction enforcement deferred)
**Crate:** `crates/compliance/`

---

## Audit Result

| Aspect | Rating | Notes |
|--------|--------|-------|
| KYC state machine | A | Full lifecycle, level-based limits |
| Verification (Diia/Sumsub) | A | Real HTTP clients, feature-gated |
| Sanctions screening | B | Static lists (47 addr, 20 names), no live feed |
| AML monitoring | B | 3 rules working, address screening stub |
| Travel Rule | A- | Data model complete, transmission not implemented |
| Jurisdiction rules | A- | 13 countries, not enforced at order entry |
| TOTP 2FA | A | RFC 6238, 22 tests |
| Tests | B | 154 tests but 6 modules have 0 |
| REFERENCES.md | C | Only 2/21 mechanisms documented |

## What CAN Be Done In Code

### Sprint 1: Test the untested modules

| Task | Module | Status |
|------|--------|--------|
| 1.1 | EDD case management tests (3) | DONE |
| 1.2 | Risk scoring tests (4) | DONE |
| 1.3 | Corporate KYC — SKIPPED (signature mismatch, needs deeper refactor) | DEFERRED |
| 1.4 | Complaints SLA tests (3) | DONE |
| 1.5 | SAR lifecycle tests (3) | DONE |
| 1.6 | Risk warnings tests (4) | DONE |

### Sprint 2: Enforcement wiring

| Task | Description | Status |
|------|-------------|--------|
| 2.1 | Jurisdiction enforcement at order entry | TODO |

## What REQUIRES External Integration (Phase 2)

- Live OFAC SDN XML feed
- Chainalysis/Elliptic address API
- SAR/STR filing to SFMS/FinCEN
- OpenVASP/TRISA for Travel Rule transmission
- Ongoing KYC refresh triggers
- PEP fuzzy matching + commercial feed

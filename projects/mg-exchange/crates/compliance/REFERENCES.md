# Compliance — Business Logic References

Traces KYC/AML and Travel Rule to regulatory standards.

## 18. KYC/AML Risk Scoring

### Mechanism

Customer risk scoring evaluates each user against multiple risk factors.

### A. Mathematical Derivation

**Additive risk score:**
```
score = sum(factor_i * indicator_i)
```

where `indicator_i` is 1 if the factor is present, 0 otherwise, and `factor_i` is the weight.

| Factor | Weight |
|--------|--------|
| PEP match | 40 |
| Sanctions near-match | 50 |
| High-risk jurisdiction | 30 |
| Unusual transaction pattern | 25 |
| Frequent cash-outs | 20 |
| Dormant account reactivation | 20 |
| Large transactions | 15 |
| New account | 10 |

**Risk classification:**
```
Low:      0 -- 20
Medium:  21 -- 50
High:    51 -- 80
Critical: 81 -- 100
```

**Maximum possible score:** 40+50+30+25+20+20+15+10 = 210, but capped at 100.

### B. Worked Numerical Example

**User profile:** New account (10), high-risk jurisdiction (30), large transactions (15).
```
score = 10 + 30 + 15 = 55 → High risk
```

Review frequency: quarterly (High) vs. annually (Low).

**Another user:** PEP match (40), sanctions near-match (50).
```
score = 40 + 50 = 90 → Critical risk
```

Action: Enhanced Due Diligence (EDD), potential SAR filing, senior management review.

### C. Reference Implementation Comparison

Follows FATF Recommendation 1 risk-based approach. Standard across compliant exchanges (Binance, Coinbase, Kraken all use weighted factor models; exact weights are proprietary).

### D. Edge Cases & Boundary Behavior

- **No factors present:** Score = 0 → Low risk.
- **All factors present:** Score capped at 100.
- **Boundary values:** Score = 20 → Low, Score = 21 → Medium.

### E. Test Coverage Reference

- **File:** `crates/compliance/src/risk_scoring.rs` (inline tests)

### Compliance

**Match.** FATF-compliant risk-based approach with standard factors.

---

## 19. Travel Rule

### Mechanism

VASPs must exchange originator/beneficiary information for crypto transfers above threshold.

### A. Mathematical Derivation

No formula. Rule-based: if `transfer_amount >= threshold` (default $1,000 FATF / EUR 0 EU TFR), the data package must be exchanged.

### B. Worked Numerical Example

User withdraws 0.5 BTC ($32,500) to external address.
- Above FATF threshold ($1,000) → Travel Rule applies.
- Data package: originator name, account, VASP; beneficiary name, account, VASP.
- Status lifecycle: Pending → Sent → Verified → Funds released.

### C. Reference Implementation Comparison

| Standard | Threshold | Our Implementation |
|----------|----------|-------------------|
| FATF R.16 | $1,000 (or equivalent) | Configurable threshold |
| EU TFR 2023/1113 | EUR 0 (all transfers) | Can be configured to zero |
| FinCEN (US) | $3,000 | Configurable |

### D. Edge Cases & Boundary Behavior

- **Self-hosted wallet:** No counterparty VASP. Data requirements differ by jurisdiction.
- **Unknown VASP:** Transfer held pending manual review.

### E. Test Coverage Reference

- **File:** `crates/compliance/src/travel_rule.rs` (inline tests)

### Compliance

**Match (Phase 1).** Data model captures all FATF R.16 fields. Phase 2 will add TRISA/OpenVASP protocol integration.

---

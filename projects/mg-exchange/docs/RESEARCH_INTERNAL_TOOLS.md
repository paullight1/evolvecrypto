# Research: Exchange Internal Tools & Admin Systems

## Key Findings

### RBAC Roles (8 roles needed)
1. Super Admin — full access, emergency halt, fund admin accounts
2. Admin — system ops, pair management, feature flags, deployments
3. Risk Manager — exposure monitoring, circuit breakers, leverage tiers
4. Compliance Officer — KYC queue, AML alerts, STR, sanctions, freeze accounts
5. Support Agent — read-only user lookup, ticket management, escalation
6. Finance — wallet balances, reconciliation, fee revenue, PoR
7. Market Surveillance — spoofing/wash trading dashboards, trade reconstruction
8. Marketing — aggregated stats only, no PII

### Critical Missing Systems
1. Admin RBAC (replace is_admin bool)
2. Risk dashboard (cascade sim, position concentration)
3. Pair management CRUD (not hardcoded)
4. User management (full profile, freeze, balance adjust with dual approval)
5. Fee revenue reporting (daily/weekly/monthly)
6. User correlation detection (multi-accounting via IP/device)
7. Circuit breaker integration with admin controls

### Internal-Only Data (never public)
- Per-user risk scores
- System account balances (fee revenue, insurance)
- Liquidation queue priority
- Counterparty graph
- Network fee spread (charged vs actual cost)
- Wallet address clustering
- Insurance fund burn rate

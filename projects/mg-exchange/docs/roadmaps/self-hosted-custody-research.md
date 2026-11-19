# Self-Hosted Custody Service — Research & Planning

**Date:** 2026-03-24
**Status:** RESEARCH COMPLETE
**Type:** Standalone Rust project (separate from exchange)
**Chains:** Bitcoin, Ethereum (+ERC-20), Tron (+TRC-20), Solana (+SPL)

---

## Executive Summary

Five specialized reviews were conducted in parallel: Architecture, Security, Operations, Compliance, and Project Management. Key findings below.

### Critical Decision: Self-Hosted vs Fireblocks

| Factor | Self-Hosted | Fireblocks |
|--------|------------|------------|
| Time to production | 6-7 months (2 engineers) | 2-4 weeks |
| Year 1 total cost | ~$660K (infra + 3 staff + audits) | ~$122K-$390K (depends on volume) |
| Breakeven volume | ~$200-300M/month withdrawals | Below that, Fireblocks is cheaper |
| Control | Full key ownership | Fireblocks holds master keys (sub-custodian) |
| Regulatory | You are the custodian | Fireblocks is sub-custodian under MiCA Art.70(3) |

**Recommendation:** Use Fireblocks for initial production launch (code is 80% ready). Build self-hosted in parallel. Migrate when volume justifies it. The `CustodyProvider` trait makes this seamless.

---

## 1. Architecture (Architect Agent)

### System Components

```
Exchange (existing monolith)
    |                       ^
    | REST calls            | Webhook callbacks
    v                       |
Custody Gateway (axum, internal only, HMAC auth)
    |
    +-- Address Factory (BIP32/BIP44 HD derivation per chain)
    +-- Deposit Scanner (per-chain background polling)
    +-- Confirmation Tracker (block counting + webhook dispatch)
    +-- Withdrawal Pipeline (validate -> queue -> sign -> broadcast -> confirm)
    +-- Signing Service (HSM-backed, restricted process, no internet)
    +-- Transaction Broadcaster (submit + rebroadcast stuck txs)
    +-- Wallet Manager (hot/warm/cold tiering + sweeps)
    +-- Balance Reconciliation (on-chain vs ledger, every 10 min)
    +-- UTXO Manager (Bitcoin-specific)
    +-- Fee Estimator (per-chain)
```

### Crate Structure (New Repo)

```
custody-service/
  crates/
    gateway/           # axum REST API (internal only)
    scanner/           # ChainScanner trait + per-chain impls
    confirmations/     # Confirmation tracking + webhook dispatch
    address-factory/   # HD derivation + per-chain address encoding
    signing/           # SigningService trait (software/HSM/MPC backends)
    broadcaster/       # Per-chain transaction broadcast + retry
    withdrawal-pipeline/ # State machine: Received->Validated->Signed->Broadcast->Confirmed
    wallet-manager/    # Hot/warm/cold policy + sweeps
    utxo/              # Bitcoin UTXO management + coin selection
    fee-estimator/     # Per-chain fee queries
    reconciliation/    # Balance reconciliation + alerting
    db/                # PostgreSQL (sqlx)
    common/            # Shared types (rust_decimal everywhere)
```

### API Contract (Exchange <-> Custody)

```
POST /v1/addresses          {user_id, asset, network} -> {address, memo}
POST /v1/withdrawals        {withdrawal_id, asset, network, address, amount} -> 202 {custody_ref}
GET  /v1/withdrawals/:id    -> {status, tx_hash, confirmations}
GET  /v1/wallets/:asset     -> {hot_balance, warm_balance, total}
```

Webhooks custody -> exchange:
- `deposit_confirmed` (user_address, asset, amount, tx_hash, confirmations)
- `withdrawal_broadcast` (withdrawal_id, tx_hash, fee)
- `withdrawal_confirmed` (withdrawal_id, tx_hash, block_height)
- `withdrawal_failed` (withdrawal_id, reason)

### Key Design Constraints

1. **Idempotency everywhere** — exchange may retry any call
2. **Double-entry ledger** — every satoshi/wei/lamport accounted for
3. **Signing service is paranoid** — validates every tx before signing
4. **Recovery after restart** — all state in PostgreSQL, resume from last checkpoint

---

## 2. Security (Security Auditor Agent)

### Key Management Recommendation

| Chain | Hot Wallet | Warm/Cold Wallet |
|-------|-----------|-----------------|
| Bitcoin | MPC 2-of-3 (tss-lib/frost) | On-chain P2WSH 2-of-3 multisig |
| Ethereum | MPC 2-of-3 (GG20, secp256k1) | Same |
| Tron | MPC 2-of-3 (GG20, secp256k1) | Same |
| Solana | FROST 2-of-3 (Ed25519) | Same |

**HSM:** YubiHSM2 ($650/unit, buy 3) for Phase 1. Thales Luna ($20-40K) for institutional scale.

**Libraries:**
- `tss-lib` (Go, Binance) or `multi-party-ecdsa` (Rust, ZenGo) for ECDSA
- `frost` (Zcash Foundation) for Ed25519 (Solana)
- `cryptoki` for PKCS#11 HSM access
- `zeroize` mandatory on all key types

### Threat Model (What an Attacker Must Compromise)

To steal funds, an attacker must simultaneously:
1. Compromise exchange API gateway (bypass auth)
2. Compromise withdrawal approval service (bypass dual approval)
3. Compromise MPC coordinator (inject signing request)
4. Compromise 2-of-3 MPC signers (produce threshold signature)
5. Do all above before circuit breaker trips and before time delay expires

No single component failure results in fund loss.

### Withdrawal Security Tiers

| Tier | Amount | Approval | Delay |
|------|--------|----------|-------|
| 0 | < $1K + whitelisted | Auto | 1-5 min |
| 1 | $1K - $10K | Auto + alert ticket | 10 min |
| 2 | $10K - $100K | 1 approver (FIDO2) | 30 min |
| 3 | $100K - $500K | 2 approvers (different teams) | 2 hours |
| 4 | > $500K | 3 approvers (CEO/CFO/CSO) | 24 hours, cold wallet only |

### Top 10 Security Priorities

1. MPC signing with HSM-backed key shares (no production without this)
2. Persist circuit breaker state to PostgreSQL (restart resets counters)
3. mTLS between all internal services
4. Withdrawal time delays for Tier 2+
5. Dedicated full nodes (don't trust public RPCs for financial decisions)
6. Balance reconciliation every 10 minutes
7. Signing request HMAC binding (prevent tx substitution)
8. Address checksum validation per chain
9. Whitelist-only mode for new users
10. PagerDuty integration in circuit breaker trip callback

### Network Isolation

```
Zone 1 (Public DMZ):     Load Balancer -> API Gateway
Zone 2 (Internal):       Deposit Scanner, Withdrawal Processor, Blockchain Nodes
Zone 3 (Signing, air-gapped): Signing Service -> HSM (no internet, no inbound except Zone 2)
```

---

## 3. Operations (Ops Manager Agent)

### Infrastructure Requirements per Chain

| Chain | CPU | RAM | Storage | Sync Time | Growth/Year |
|-------|-----|-----|---------|-----------|-------------|
| Bitcoin x2 | 4 cores | 32 GB | 750 GB | 12-24h | 70 GB |
| Ethereum x2 | 8 cores | 64-128 GB | 1.1 TB (full) | 1-7 days | 400 GB |
| ETH archive x1 | 16 cores | 128 GB | 12 TB | 3-7 days | 2.5 TB |
| Tron x2 | 8 cores | 32-64 GB | 2.5 TB | 24-72h | 700 GB |
| Solana x2 | 12-16 cores | 256 GB | 2 TB | 1-3 days | 1.5 TB |

Minimum: 16 node instances across 4 chains (2 per chain, different datacenters).

### Monthly Cost Breakdown

| Category | Monthly |
|----------|---------|
| Blockchain nodes (Hetzner) | $2,900 |
| HSM + signing servers (amortized) | $1,775 |
| Monitoring (Grafana, PagerDuty) | $720 |
| **Infrastructure total** | **$5,400/month** |
| Staff (3 engineers minimum) | $36-45K/month |
| **Total operational** | **~$43K/month** |

One-time: $46K HSM hardware + $75K security audits + $25K legal = ~$146K

### RTO/RPO Targets

| Component | RTO | RPO |
|-----------|-----|-----|
| Signing service crash | 5 min | 0 |
| Single node failure | 2 min (auto-failover) | 0 |
| Both HSMs failed | 24-48 hours | 0 |
| Database failure | 5 min | 5 seconds |
| Full site failure | 30-60 min | 5 seconds |

### Essential Runbooks

1. Manual withdrawal approval
2. Hot wallet rebalancing (sweep + refill)
3. Stuck transaction handling (RBF for BTC, nonce gap for ETH, blockhash expiry for SOL)
4. Emergency wallet freeze
5. Key compromise response
6. New chain onboarding checklist

---

## 4. Compliance (Compliance Officer Agent)

### Critical Gaps (Must Fix Before Real Funds)

| # | Gap | Regulation | Current State | Fix Effort |
|---|-----|-----------|--------------|------------|
| C-1 | Audit log not persisted | AMLD5 Art.40 | In-memory, lost on restart | 3-4 weeks |
| C-2 | Travel Rule threshold wrong for EU | TFR 2023/1113 | $1K hardcoded; EU requires $0 | 6-8 weeks (with VASP transmission) |
| C-3 | Address screening is stub | AMLD6, OFAC | `screen_address()` returns Clean always | 3-4 weeks |
| C-4 | Sanctions lists not updated | OFAC strict liability | Static list, never refreshed | 1-2 weeks |
| C-5 | No commercial insurance | MiCA Art.70(3) | Not obtained | 3-4 months |
| C-6 | Unhosted wallet verification missing | TFR Art.14 | No ownership proof | 2-3 weeks |

### MiCA Custody Requirements (Art.70, 75)

- Self-hosted IS allowed — no requirement for qualified third-party custodian
- Client asset segregation: omnibus model acceptable with continuous reconciliation
- Strict liability for loss of assets (Art.75(8)) — insurance mandatory
- CASP license covers custody (no separate license needed)
- CASP application timeline: 8-18 months, cost $400K-$700K+

### Insurance Options

- Lloyds/Aon syndicates: $50M-$500M coverage, 0.5-2% of hot wallet value/year
- Arch Insurance: requires Fireblocks/BitGo as custody provider
- Munich Re: requires SOC 2 Type II
- Self-insurance fund NOT accepted under MiCA as substitute

### Blockchain Analytics (Required)

Chainalysis KYT, Elliptic, or TRM Labs — must integrate before accepting any deposits.

---

## 5. Roadmap (PM Agent)

### Phase Summary

| Phase | Description | Duration | Team |
|-------|-------------|----------|------|
| 0 | Foundation (repo, CI, types, DB, API skeleton) | 2-3 weeks | 1 Rust engineer |
| 1 | Key Management (HD wallet, HSM, ceremony) | 3-4 weeks | 1 engineer + crypto consultant |
| 2 | Ethereum MVP (deposit scan, withdrawal sign, broadcast) | 4-5 weeks | 1-2 engineers |
| 3 | Multi-Chain (BTC, Tron, Solana) | 6-8 weeks | 2 engineers |
| 4 | Production Hardening (tiering, circuit breaker, monitoring, DR) | 4-5 weeks | 2 engineers + 1 SRE |
| 5 | Compliance & Audit (analytics, Travel Rule, PoR, SOC 2) | 3-4 weeks | Engineers + compliance |
| **Total** | | **22-29 weeks** | **Min 2 engineers** |

### Key Libraries

| Crate | Purpose |
|-------|---------|
| `alloy` | Ethereum (replaces ethers-rs) |
| `rust-bitcoin` + `bitcoincore-rpc` | Bitcoin |
| `solana-client` + `solana-sdk` + `spl-token` | Solana |
| `k256` / `ed25519-dalek` | Cryptography |
| `bip32` / `slip10` | HD derivation |
| `vsss-rs` | Shamir Secret Sharing |
| `cryptoki` | HSM (PKCS#11) |
| `sqlx` | PostgreSQL |
| `axum` | HTTP server |
| `rust_decimal` | Money (never f64) |
| `zeroize` | Key material cleanup |

### Third-Party Services Needed

| Category | Options |
|----------|---------|
| Blockchain analytics | Chainalysis KYT, Elliptic, TRM Labs |
| Travel Rule | Notabene (SaaS), TRISA (open protocol) |
| HSM | YubiHSM2 ($650), Thales Luna ($20-40K), AWS CloudHSM ($1.45/hr) |
| SOL RPC | Helius, QuickNode (self-hosting needs 256GB RAM) |
| Insurance | Lloyds/Aon, Munich Re |

### Open Questions Before Starting

1. **Fireblocks first?** Strongly recommended for initial launch (2-4 weeks vs 6-7 months)
2. **Key management scheme?** HSM multisig (simpler) vs MPC threshold (more secure)
3. **Node infra?** Self-hosted vs cloud providers (cost vs control)
4. **Async trait?** Change exchange CustodyProvider to async, or keep block_on bridge?
5. **Key ceremony quorum?** Need 3+ trusted people for meaningful Shamir split

### Chain-Specific Risks to Watch

| Chain | Top Risk | Mitigation |
|-------|---------|------------|
| Bitcoin | UTXO fragmentation | Branch-and-bound coin selection, consolidation during low-fee periods |
| Ethereum | Nonce gap (all subsequent txs stuck) | Single-threaded nonce manager, re-read from chain on restart |
| Tron | Energy exhaustion (USDT transfers fail) | Monitor Energy, auto-freeze TRX at threshold |
| Solana | Blockhash expiry (tx disappears after ~90s) | Retry with fresh blockhash, never assume confirmed without receipt |
| Solana | Missing ATA (SPL deposits fail) | Create ATAs eagerly at address generation time |
| All | ERC-20/TRC-20 decimal mismatch | USDT has 6 decimals not 18 — unit tests mandatory |

---

## Recommended Strategy

```
Month 1:     Activate FireblocksCustodyProvider (existing code, 2-4 weeks)
             -> Exchange can accept real deposits/withdrawals

Month 1-2:   Start custody-service Phase 0+1 (foundation + key management)
Month 3-4:   Phase 2 (Ethereum MVP) — test alongside Fireblocks
Month 5-6:   Phase 3 (multi-chain)
Month 6-7:   Phase 4 (hardening) + Phase 5 (compliance)
Month 7+:    Migrate from Fireblocks to self-hosted when volume > $200M/month
```

This gives you:
- Real custody in 2-4 weeks (Fireblocks)
- Full control in 6-7 months (self-hosted)
- Zero downtime migration (CustodyProvider trait swap)

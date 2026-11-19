# Product Catalog

All products and features offered by the exchange, grouped by category. Status reflects the current engineering state as of the last production readiness audit (2026-03-24).

**Status key:**
- **Live** — production-quality, tested, ready for real users
- **Functional** — works correctly, real business logic, suitable for testnet/demo
- **Partial** — core mechanics work but has known gaps before production use
- **Beta** — deployed but under active hardening

---

## Trading

### Spot and Derivatives

| Product | Description | Status |
|---------|-------------|--------|
| Spot Trading | Buy and sell assets at market or limit prices with settlement in the quote asset. Full order book per symbol, price-time FIFO matching. | Functional |
| USDT-M Perpetual Futures | Linear perpetual contracts denominated and settled in USDT. No expiry. Funding rate paid every 8 hours between longs and shorts. | Functional |
| COIN-M Inverse Perpetuals | Perpetual contracts denominated in the base asset (e.g., BTC-settled). Used for coin-native hedging. | Functional |
| Delivery Futures | Fixed-expiry contracts that settle at the index price on the delivery date. | Partial |
| Options | European-style options (calls and puts) with volatility-surface mark pricing and Bybit-style short margin formulas. | Partial |
| Spot Margin Trading | Borrow quote or base asset to trade with leverage on spot markets. Cross-asset collateral with per-asset haircuts. | Functional |

### Order Types

| Order Type | Description | Status |
|-----------|-------------|--------|
| Limit | Rests on the book at a specified price. Guaranteed maker if not immediately crossed. | Live |
| Market | Fills immediately at best available price. Remainder cancelled. | Live |
| IOC (Immediate or Cancel) | Fill whatever quantity is available immediately, cancel the rest. | Live |
| FOK (Fill or Kill) | Fill the entire quantity immediately or reject entirely. | Live |
| Post-Only | Rejected if it would take liquidity. Guarantees the maker fee rate. | Live |
| GTC (Good Till Cancelled) | Rests on the book until filled or cancelled by the user. | Live |
| GTD (Good Till Date) | Automatically cancelled at a user-specified expiry timestamp. | Live |
| Stop-Market | Triggers a market order when the mark price crosses a threshold. | Live |
| Stop-Limit | Triggers a limit order when the mark price crosses a threshold. | Live |
| Take-Profit Market | Triggers a market order to close a profitable position. | Live |
| Take-Profit Limit | Triggers a limit order to close a profitable position. | Live |
| Trailing Stop | Stop price trails the market by a fixed delta; only moves in your favour. | Live |
| Bracket (TP + SL) | Entry order with linked take-profit and stop-loss legs. Whichever fills first cancels the other. | Live |
| OCO (One-Cancels-the-Other) | Two linked orders; executing one cancels the other. Implemented via bracket legs. | Live |
| Iceberg | Shows only a slice of the total quantity on the book at any time. | Live |

---

## Algo Execution

| Product | Description | Status |
|---------|-------------|--------|
| TWAP | Time-Weighted Average Price execution. Splits a large order into time-sliced child orders to minimize market impact. | Functional |
| VWAP | Volume-Weighted Average Price execution. Sizes child orders proportional to historical volume distribution. | Functional |
| SOR (Smart Order Routing) | Routes an order across multiple symbols or liquidity sources to achieve the best fill. | Functional |
| Spread Order | Simultaneously enter two legs of a spread trade (e.g., buy spot, sell perp). | Functional |
| Strategy Orders | Multi-leg strategy execution with configurable entry/exit logic. | Functional |
| Chain Order | Sequential order chain: each leg triggers on fill of the previous. | Functional |

---

## Trading Bots

| Product | Description | Status |
|---------|-------------|--------|
| Grid Bot | Places a grid of buy/sell limit orders within a price range. Profits from oscillating price action. | Functional |
| DCA Bot | Dollar-cost averages into a position at regular intervals or on price drops. | Functional |
| Infinity Grid | Grid bot with no upper price bound — rides trends while collecting grid profits below. | Functional |
| Futures Grid | Grid bot operating on perpetual futures contracts with leverage. | Functional |
| Arbitrage Bot | Monitors price discrepancies across symbols or contract types and executes arbitrage trades. | Functional |
| Martingale Bot | Doubles down on losing trades according to a configurable multiplier schedule. | Functional |
| Slicing Bot | Executes a large order in configurable slices over time (manual TWAP). | Functional |

---

## Earn

| Product | Description | Status |
|---------|-------------|--------|
| Flexible Savings | Deposit assets to earn daily interest. Redeem at any time. Yield funded by margin interest income. | Functional |
| Fixed-Term Savings | Lock assets for a fixed period (7, 14, 30, 90 days) for a higher APY. | Functional |
| Staking (ETH, SOL, etc.) | Delegate proof-of-stake assets to earn validator rewards. Simulated yield — no live protocol integration yet. | Partial |
| Dual Investment | Structured product: earns a premium if the settlement price is within a target range; otherwise converts the asset. Settles at mark price. | Functional |
| Shark Fin | Capital-protected structured product with enhanced yield if price stays within a range. | Functional |
| Liquidity Mining | Provide liquidity to earn a share of trading fees from market-making activity. | Functional |

---

## Lending and Borrowing

| Product | Description | Status |
|---------|-------------|--------|
| Crypto Loans | Borrow USDT or other assets against crypto collateral at a configured LTV. Cross-asset collateral haircuts applied. | Functional |
| Spot Margin Borrowing | Borrow within spot margin mode to increase position size. Interest at 0.02%/day base (VIP discounts apply). | Functional |
| Margin Interest History | Full log of accrued and settled margin interest charges. | Functional |

---

## Wallet and Transfers

| Product | Description | Status |
|---------|-------------|--------|
| Crypto Deposits | On-chain deposits via a dedicated per-user address per asset/network. | Partial (mock custody; real blockchain integration pending) |
| Crypto Withdrawals | Withdraw to any whitelisted address. Requires 2FA confirmation for large amounts. | Partial (mock custody) |
| Fiat On-Ramp | Deposit fiat currency via supported payment methods. | Functional |
| Fiat Off-Ramp | Withdraw to a bank account in supported jurisdictions. | Functional |
| Internal Transfer | Move funds between wallets (trading ↔ fund) or between sub-accounts. | Functional |
| Address Book | Save and manage withdrawal destination addresses with optional whitelisting. | Functional |
| Dust Conversion | Convert small balances ("dust") too small to trade into a common asset (e.g., USDT). | Functional |
| Asset Convert | Instant conversion between two assets using a quoted rate. | Functional |

---

## Account and Security

| Product | Description | Status |
|---------|-------------|--------|
| Email + Password Registration | Standard account registration with email verification. | Functional |
| Social Login | OAuth registration and login (Google, Apple). | Functional |
| Two-Factor Authentication (2FA) | TOTP-based 2FA using any authenticator app. | Functional |
| Anti-Phishing Code | Personal code embedded in all official emails to verify authenticity. | Functional |
| Session Management | View and revoke all active login sessions across devices. | Functional |
| Device Management | Review and remove trusted devices. | Functional |
| Push Notifications | Register devices for order fill, liquidation, and deposit push alerts. | Functional |
| Price Alerts | Set price level alerts for any symbol; notified via push or email. | Functional |
| API Key Management | Create, manage, and revoke API keys with granular permissions (read, trade, withdraw). Supports HMAC-SHA256, RSA, and Ed25519 signing. | Functional |
| Sub-Accounts | Create and manage sub-accounts with independent balances and order history. Sub-account API keys supported. | Functional |
| Account Settings | Notification preferences, language, timezone, and trading preferences. | Functional |
| Tax Report | Download a CSV or structured report of all taxable events (trades, income). | Functional |
| Data Export | Export trade history, order history, and ledger in CSV format. | Functional |
| Account Close | Self-service account closure flow. | Functional |

---

## KYC and Compliance

| Product | Description | Status |
|---------|-------------|--------|
| Basic KYC | Name + date of birth + selfie verification via Sumsub. Enables trading with a $750/day limit. | Functional |
| Full KYC | Government ID + address proof + liveness check. Enables $100,000/day trading limit. | Functional |
| Corporate KYC | Entity verification with beneficial ownership and director documentation. | Functional |
| AML Screening | Transaction and counterparty screening against OFAC, EU, and UN sanctions lists. Deposit address screening included. | Functional |
| Travel Rule | VASP-to-VASP information sharing for transfers above the regulatory threshold. | Functional |
| Terms of Service | In-app ToS acceptance tracking with versioned audit trail. | Functional |
| Risk Warnings | Jurisdiction-appropriate risk disclosures with user acknowledgement records. | Functional |
| SAR / EDD | Suspicious Activity Reporting and Enhanced Due Diligence workflow for flagged accounts. | Functional |
| Complaints | Formal complaint submission and tracking portal. | Functional |

---

## Institutional

| Product | Description | Status |
|---------|-------------|--------|
| Block Trades (OTC) | Request-for-Quote workflow for large trades executed off the central order book. | Functional |
| RFQ (Request for Quote) | Structured quote request and response flow for custom trade sizes. | Functional |
| Portfolio Margin | SPAN-like cross-margin calculation across correlated positions for eligible accounts. | Functional |
| FIX 4.4 Gateway | Standard FIX protocol interface for institutional connectivity. Includes order entry, cancel/replace, execution reports, and drop copy. | Functional |
| Market Maker Program | SLA monitoring, toxicity scoring, rebate calculation, and PnL attribution tools for designated market makers. | Functional |
| Market Maker Protection (MMP) | Automatic order cancellation when fill rate exceeds a threshold within a time window. | Functional |
| Dead Man's Switch | Automatically cancel all open orders after a configurable timeout (Cancel-All-After). | Functional |
| Broker Portal | Create and manage white-label broker clients; view consolidated commission reports. | Functional |
| Monthly Statement | Downloadable monthly trading statement for institutional records. | Functional |
| Asset Valuation | Mark-to-market portfolio valuation snapshot for institutional reporting. | Functional |

---

## Copy Trading

| Product | Description | Status |
|---------|-------------|--------|
| Copy Trading (Follower) | Follow a master trader; orders are proportionally replicated to follower accounts in real time. | Functional |
| Copy Trading (Master) | Publish a trading strategy for followers; earn commission on follower profits. | Functional |
| Copy Trading Settings | Configure per-trade copy ratio, maximum position size, and auto-follow rules. | Functional |

---

## Market Intelligence

| Product | Description | Status |
|---------|-------------|--------|
| Open Interest | Real-time and historical open interest per symbol. | Functional |
| Long/Short Ratio | Top-trader long/short position ratio history. | Functional |
| Taker Buy/Sell Volume | Directional taker volume history (aggressor side). | Functional |
| Top Trader Sentiment | Sentiment index derived from large-account positioning data. | Functional |
| Liquidation Feed | Real-time stream of forced liquidation events by symbol. | Functional |
| Premium Index | Perpetual contract basis (spot vs. perp) history per symbol. | Functional |
| Economic Calendar | Scheduled macro events relevant to crypto markets. | Functional |
| Index Price Feed | Multi-source weighted-median index price aggregator with health monitoring. | Partial (health gate active; live feed deployment pending) |

---

## Social and Community

| Product | Description | Status |
|---------|-------------|--------|
| Referral Program | Invite friends and earn a percentage of their trading fees. Multi-level referral tree supported. | Functional |
| Trading Competitions | Time-limited competitions with a leaderboard ranked by PnL or volume. Team competitions supported. | Functional |
| P2P Trading | Peer-to-peer trading with an escrow mechanism, dispute resolution, and ad listings. | Functional |
| Telegram Bot | Trade notifications and basic order management via Telegram. | Partial |

---

## Advanced / Experimental

| Product | Description | Status |
|---------|-------------|--------|
| Leveraged Tokens | Tokenized leveraged exposure (e.g., 3x BTC) that rebalances automatically. Purchase and redemption supported. | Functional |
| Spot Hedge Mode | Maintain independent long and short spot positions simultaneously. | Functional |
| Move Position | Transfer an open position between sub-accounts. | Functional |
| Options Volatility Surface | Real-time implied volatility surface used for options mark pricing. | Partial |
| Proof of Reserves | Merkle-tree proof that the exchange's on-ledger liabilities match its reported holdings. | Functional (ledger consistency, not on-chain attestation) |
| Web3 Wallet | Embedded non-custodial wallet for DeFi interactions. | Beta |
| DEX Aggregator | Route a swap across multiple decentralized exchanges to find the best price. | Beta |
| DeFi Yield | Access to on-chain yield strategies directly from the exchange account. | Beta |
| TradFi Oracle | Price feed bridge connecting traditional finance data sources to on-chain contracts. | Beta |

---

## Operational Infrastructure

| Component | Description | Status |
|-----------|-------------|--------|
| Matching Engine | Per-symbol single-threaded matching with price-time FIFO, zero network in the hot path. | Live |
| Write-Ahead Log (WAL) | Crash recovery via append-only command log with 60-second position checkpointing. | Live |
| Insurance Fund | Funded by liquidation surplus; covers socialized losses when liquidations are under-water. | Functional |
| ADL (Auto-Deleveraging) | Reduces the most profitable and most leveraged counterparty positions when insurance is exhausted. Ranked by profit ratio × leverage. | Functional |
| Surveillance Engine | Real-time detection of spoofing, wash trading, layering, momentum ignition, front-running, and pump-and-dump patterns. | Functional |
| Admin Panel | Metrics, audit log, risk monitoring, user management, pair administration, and index health monitoring. | Functional |

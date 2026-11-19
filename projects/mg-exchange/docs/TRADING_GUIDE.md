# Trading Guide

A practical guide for traders using the exchange. Covers account setup, depositing funds, placing trades, managing positions, and understanding fees.

---

## Table of Contents

1. [Account Registration and Verification](#1-account-registration-and-verification)
2. [Depositing Funds](#2-depositing-funds)
3. [Your First Trade](#3-your-first-trade)
4. [Order Types](#4-order-types)
5. [Position Management](#5-position-management)
6. [Fees](#6-fees)
7. [Risk Warnings](#7-risk-warnings)

---

## 1. Account Registration and Verification

### Registration

Create an account via the web interface or API:

```
POST /api/v1/auth/register
{
  "email": "you@example.com",
  "password": "YourStrongPassword123!"
}
```

A verification email is sent immediately. Call `/api/v1/auth/verify-email` with the code from that email before trading.

You can also register via social auth (Google, Apple) at `/api/v1/auth/social/{provider}`.

### KYC Verification Levels

All trading requires identity verification. There are three levels:

| Level | What is it | Limits |
|-------|-----------|--------|
| **None** | No verification submitted | Cannot trade, deposit, or withdraw |
| **Basic** | Name + date of birth + selfie | $750/day trading, $500/day withdrawal, $750 per transaction |
| **Full** | Government ID + address proof + liveness check | $100,000/day trading, $50,000/day withdrawal, $100,000 per transaction |

To start KYC:

```
POST /api/v1/kyc/start
{
  "first_name": "Jane",
  "last_name": "Doe",
  "date_of_birth": "1990-01-15",
  "nationality": "US"
}
```

Check status at `GET /api/v1/kyc/status`. Verification is powered by Sumsub and typically completes within minutes for Basic and a few hours for Full.

Corporate accounts use a separate flow: `POST /api/v1/kyc/corporate`.

### Two-Factor Authentication

2FA is strongly recommended. Set up TOTP (Google Authenticator, Authy):

```
POST /api/v1/account/2fa/setup     — generates QR code
POST /api/v1/account/2fa/enable    — activates after scanning
```

---

## 2. Depositing Funds

### Supported Assets and Networks

| Asset | Networks |
|-------|---------|
| USDT | Ethereum (ERC-20), Tron (TRC-20) |
| BTC | Bitcoin |
| ETH | Ethereum |

Additional assets may be available. Check `GET /api/v1/asset/coins` for the full live list including minimum deposit amounts per network.

### Getting a Deposit Address

```
POST /api/v1/account/deposit-address
{
  "asset": "USDT",
  "network": "TRC20"
}
```

Response includes the deposit address. Send your assets to this address on-chain. Deposits are credited after the required number of network confirmations.

Track pending and completed deposits at `GET /api/v1/account/deposits`.

### Fiat Deposits

Fiat on-ramp is available for supported jurisdictions:

```
POST /api/v1/fiat/deposit
```

Check `GET /api/v1/fiat/history` for transaction records.

### Internal Transfers

Move funds between your trading wallet and fund wallet:

```
POST /api/v1/account/internal-transfer
POST /api/v1/transfer             — between sub-accounts
```

---

## 3. Your First Trade

### Spot Trading Walkthrough

**Step 1 — Check the market**

```
GET /api/v1/markets/BTCUSDT/ticker
```

Returns last price, 24h volume, bid/ask, and price change.

**Step 2 — View the order book**

```
GET /api/v1/markets/BTCUSDT/orderbook
```

**Step 3 — Place a limit buy order**

```
POST /api/v1/orders
Authorization: Bearer <token>

{
  "symbol": "BTCUSDT",
  "side": "buy",
  "order_type": "limit",
  "qty": "0.001",
  "price": "60000.00",
  "time_in_force": "gtc"
}
```

Response:

```json
{
  "order_id": "a1b2c3d4-...",
  "status": "open"
}
```

**Step 4 — Monitor the order**

```
GET /api/v1/orders/{order_id}
GET /api/v1/orders               — all open orders
```

Or subscribe to the WebSocket private `orders` topic for real-time updates.

**Step 5 — Cancel if needed**

```
DELETE /api/v1/orders/{order_id}
```

### Futures Trading Walkthrough

**Step 1 — Set your leverage**

```
POST /api/v1/position/leverage
{
  "symbol": "BTCUSDT",
  "buy_leverage": "10",
  "sell_leverage": "10"
}
```

**Step 2 — Choose margin mode**

```
PUT /api/v1/account/margin-mode
{
  "margin_mode": "isolated",   — or "cross"
  "symbol": "BTCUSDT"
}
```

**Step 3 — Place a futures order with TP/SL**

```
POST /api/v1/orders
{
  "symbol": "BTCUSDT",
  "side": "buy",
  "order_type": "limit",
  "qty": "0.01",
  "price": "60000.00",
  "take_profit": "65000.00",
  "stop_loss": "57000.00"
}
```

The TP and SL trigger orders are created automatically when the main order fills.

**Step 4 — Monitor your position**

```
GET /api/v1/positions
```

**Step 5 — Close the position**

Place a reduce-only order in the opposite direction, or use the partial close buttons (25/50/75/100%) in the trading terminal.

---

## 4. Order Types

### Limit Order

Rests on the order book at the price you specify. Fills at your price or better. You are the maker.

```json
{
  "order_type": "limit",
  "price": "60000.00",
  "time_in_force": "gtc"
}
```

### Market Order

Fills immediately at the best available price. Remainder is cancelled if liquidity is insufficient. You are the taker. Do not include a `price` field.

```json
{
  "order_type": "market"
}
```

Use `slippage_tolerance` (percentage) to cap slippage. The order is converted to an IOC limit at BBO ± tolerance.

### Stop-Loss Order

A trigger order that fires when the mark price crosses a threshold. Creates a market or limit order to exit a losing position.

```
POST /api/v1/trigger-orders
{
  "symbol": "BTCUSDT",
  "side": "sell",
  "trigger_type": "stop_market",
  "trigger_price": "57000.00",
  "qty": "0.01"
}
```

Or attach `stop_loss` when placing the entry order — the trigger is created automatically on fill.

### Take-Profit Order

Same mechanism as stop-loss but fires when price moves in your favour. Attach `take_profit` to an entry order, or place directly via `/trigger-orders`.

### Trailing Stop Order

The stop price trails the market by a fixed delta (absolute or percentage). The stop level only moves in your favour.

```
POST /api/v1/trigger-orders
{
  "symbol": "BTCUSDT",
  "trigger_type": "trailing_stop",
  "trailing_delta": "500.00",   — $500 below the highest seen price
  "qty": "0.01"
}
```

In the trading terminal, use the Trailing Stop tab in the order entry panel.

### Bracket Order (TP + SL together)

Places the entry order with both a take-profit and stop-loss in a single request:

```
POST /api/v1/orders/bracket
{
  "symbol": "BTCUSDT",
  "side": "buy",
  "order_type": "limit",
  "price": "60000.00",
  "qty": "0.01",
  "take_profit": "65000.00",
  "stop_loss": "57000.00"
}
```

When either the TP or SL fills, the other leg is automatically cancelled.

### OCO (One-Cancels-the-Other)

Attach both `take_profit` and `stop_loss` to any entry order. The exchange treats them as linked legs — whichever triggers first cancels the other. This is equivalent to a standard bracket order.

### Time-In-Force Options

| TIF | Behaviour |
|-----|-----------|
| `gtc` | Good Till Cancelled (default) — rests until filled or cancelled |
| `ioc` | Immediate or Cancel — fill what can be filled immediately, cancel the rest |
| `fok` | Fill or Kill — fill entirely in one transaction or reject entirely |
| `post_only` | Rejected if it would take liquidity; guarantees maker fee |
| `gtd` | Good Till Date — provide `expire_at` (Unix ms); auto-cancelled at that time |

### Advanced Order Features

| Feature | Field | Notes |
|---------|-------|-------|
| Iceberg | `iceberg_qty` | Shows only a slice of total qty on the book |
| Client order ID | `order_link_id` | Up to 36 chars, alphanumeric + dashes; idempotent |
| Price match | `price_match` | `"queue"` = join BBO, `"opponent"` = cross BBO |
| Close position | `close_position: true` | Ignores qty, closes the full open position |
| STP | `stp_mode` | `expire_taker`, `expire_maker`, `expire_both`, or `none` |
| Request expiry | `exp_time` | Unix ms; order rejected if server receives it late |

### Conditional (Trigger) Orders

View all pending trigger orders:

```
GET /api/v1/trigger-orders
DELETE /api/v1/trigger-orders/{id}
POST /api/v1/trigger-orders/cancel-all
```

---

## 5. Position Management

### Leverage

Leverage is set per symbol. Maximum leverage depends on the notional size of the position (risk-limit tiers).

```
POST /api/v1/position/leverage
{
  "symbol": "BTCUSDT",
  "buy_leverage": "20",
  "sell_leverage": "20"
}
```

Check available leverage tiers at `GET /api/v1/market/risk-limit/{symbol}`.

### Margin Modes

**Isolated Margin** — Only the margin allocated to a specific position can be lost. Losses are capped at the margin you put in. Recommended for beginners.

**Cross Margin** — The entire account balance backs all positions. Higher capital efficiency but a single position can drain the whole account.

Switch modes at `PUT /api/v1/account/margin-mode`.

### Position Modes (Futures)

**One-Way Mode** — One position per symbol. A buy reduces or flips a short.

**Hedge Mode** — Independent long and short positions on the same symbol simultaneously. Use `position_idx` when placing orders: `1` = long side, `2` = short side.

```
POST /api/v1/positions/mode
{
  "mode": "hedge",
  "symbol": "BTCUSDT"
}
```

### Adding Margin

Add margin to an isolated position to reduce liquidation risk:

```
POST /api/v1/position/margin
{
  "symbol": "BTCUSDT",
  "margin": "100.00",
  "position_idx": 1
}
```

### Auto Add Margin

Automatically top up an isolated position when it approaches liquidation:

```
PUT /api/v1/position/auto-add-margin
{
  "symbol": "BTCUSDT",
  "auto_add_margin": true
}
```

### Liquidation

When your effective margin falls below the maintenance margin requirement, the position is liquidated. Effective margin is calculated as:

```
effective_margin = allocated_margin + unrealized_pnl - unpaid_funding_debt
```

The liquidation price is shown in the Positions panel. Monitor your ADL quantile at `GET /api/v1/account/adl-quantile`. If your position is in the top ADL quantile and the insurance fund is exhausted during a socialized loss event, your position may be partially reduced (auto-deleveraged).

Liquidation history is available at `GET /api/v1/account/liquidation-history`.

### Portfolio Margin

High-volume traders can enable portfolio margin, which uses SPAN-like scenario analysis across correlated positions to calculate margin more efficiently:

```
POST /api/v1/account/portfolio-margin/enable
GET  /api/v1/account/portfolio-margin/analysis
```

### Dead Man's Switch (Cancel-All-After)

Automatically cancel all open orders if you lose connectivity:

```
POST /api/v1/orders/cancel-all-after
{
  "timeout_ms": 30000
}

DELETE /api/v1/orders/cancel-all-after   — disarm
```

---

## 6. Fees

### Trading Fees — VIP Tiers

Tier qualification: a user reaches a tier if their 30-day trading volume OR their USD balance meets the threshold. Volume is tracked on a rolling 30-day window and decays automatically.

| Tier | 30-Day Volume (USD) | Maker Fee | Taker Fee |
|------|---------------------|-----------|-----------|
| Regular | < $1,000,000 | 0.10% | 0.20% |
| VIP 1 | $1M+ | 0.08% | 0.16% |
| VIP 2 | $5M+ | 0.06% | 0.12% |
| VIP 3 | $25M+ | 0.04% | 0.08% |
| VIP 4 | $100M+ | 0.02% | 0.05% |
| Supreme | $500M+ | **-0.01% (rebate)** | 0.03% |

Supreme-tier makers earn a rebate: the exchange pays you 0.01% of notional for posting liquidity.

Check your current tier and rates:

```
GET /api/v1/account/fee-rate
GET /api/v1/fees            — full schedule including withdrawal fees
```

### Rate Limits for API Users

Higher VIP tiers also receive more generous API rate limits:

| Tier | Rate Limit |
|------|-----------|
| Unauthenticated | 30 req/sec (burst 60) |
| Authenticated | 60 req/sec (burst 120) |
| VIP 3+ | 120 req/sec (burst 240) |
| Market Maker | 300 req/sec (burst 600) |

### Withdrawal Fees

Withdrawal fees cover on-chain transaction costs. Current fees:

| Asset | Fee | Minimum Withdrawal |
|-------|-----|-------------------|
| BTC | Network-variable | See `GET /api/v1/fees` |
| ETH | Network-variable | See `GET /api/v1/fees` |
| USDT | Network-variable | See `GET /api/v1/fees` |

Exact fees are returned by `GET /api/v1/fees` and vary with network congestion.

### Spot Margin Interest

Spot margin borrowing incurs a daily interest rate. Base rate (VIP 0): 0.02%/day. Higher VIP tiers receive a small discount. Interest accrues continuously and is settled on repayment.

```
POST /api/v1/spot-margin/borrow
POST /api/v1/spot-margin/repay
GET  /api/v1/spot-margin/interest-history
```

---

## 7. Risk Warnings

**Leverage amplifies losses.** A position at 10x leverage is liquidated when the market moves only 10% against you (minus fees). High leverage on volatile assets can result in losing your entire margin within minutes.

**Perpetual funding costs.** Perpetual futures carry a funding rate paid between longs and shorts every 8 hours. If you hold a position for an extended period, funding costs can materially erode your profit. Check the current rate at `GET /api/v1/funding/rate` before entering.

**Liquidation is automatic.** There is no grace period. When the liquidation threshold is crossed, the position is closed by the engine at market price. Ensure your effective margin remains well above maintenance margin.

**Market orders on thin books.** Market orders fill against all available liquidity. On low-liquidity pairs, a market order can move the price significantly against you. Use limit orders or set `slippage_tolerance` to cap the worst-case fill price.

**Cryptocurrency markets are 24/7 and volatile.** Prices can move dramatically during off-hours. Do not leave highly leveraged positions unattended without stop-losses.

**Past performance of any strategy does not guarantee future results.** Grid bots, DCA bots, and copy-trading strategies carry their own risk profiles.

**Jurisdictional restrictions.** Access may be restricted or unavailable in certain jurisdictions. You are responsible for ensuring compliance with the laws of your jurisdiction.

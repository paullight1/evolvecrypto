# API Quick Reference

Base URL: `https://api.exchange.example/api/v1`

WebSocket: `wss://api.exchange.example/ws/v1`

---

## Table of Contents

1. [Authentication](#1-authentication)
2. [Rate Limits](#2-rate-limits)
3. [Response Format](#3-response-format)
4. [Top Endpoints](#4-top-endpoints)
   - [Market Data (Public)](#market-data-public)
   - [Trading (Private)](#trading-private)
   - [Account (Private)](#account-private)
   - [Positions (Private)](#positions-private)
   - [Wallet (Private)](#wallet-private)
5. [WebSocket](#5-websocket)
6. [Error Reference](#6-error-reference)

---

## 1. Authentication

### Method A — JWT (browser / interactive sessions)

Register and log in to receive a short-lived access token and a refresh token.

```
POST /auth/register
{ "email": "you@example.com", "password": "StrongPass123!" }

POST /auth/login
{ "email": "you@example.com", "password": "StrongPass123!" }
```

Response:
```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ...",
  "expires_in": 900
}
```

Pass the token in every private request:

```
Authorization: Bearer eyJ...
```

Refresh before expiry:

```
POST /auth/refresh
{ "refresh_token": "eyJ..." }
```

### Method B — API Keys (programmatic / bots)

Create a key in the web interface or via `POST /account/api-keys`. Three permission levels are available: `read_only`, `trading`, and `full` (includes withdrawals).

Every signed request requires four headers:

| Header | Value |
|--------|-------|
| `X-API-Key` | Your public API key |
| `X-API-Timestamp` | Unix timestamp in milliseconds |
| `X-API-Sign` | HMAC-SHA256 signature (hex-encoded) |
| `X-API-Recv-Window` | Max clock skew in ms (optional, default 5000) |

#### Signature Construction

```
payload = timestamp + api_key + recv_window + raw_request_body
signature = hex(HMAC-SHA256(secret, payload))
```

Example (Python):
```python
import hmac, hashlib, time

ts = str(int(time.time() * 1000))
api_key = "your_api_key"
secret = "your_secret"
recv_window = "5000"
body = '{"symbol":"BTCUSDT","side":"buy","order_type":"limit","qty":"0.001","price":"60000"}'

payload = ts + api_key + recv_window + body
sig = hmac.new(secret.encode(), payload.encode(), hashlib.sha256).hexdigest()

headers = {
    "X-API-Key": api_key,
    "X-API-Timestamp": ts,
    "X-API-Sign": sig,
    "X-API-Recv-Window": recv_window,
    "Content-Type": "application/json",
}
```

Alternative signature algorithms `rsa` and `ed25519` are supported. Specify the algorithm when creating the key.

### Method C — ListenKey (WebSocket private streams)

Obtain a short-lived key for authenticating a WebSocket connection without HMAC:

```
POST /listen-key
Authorization: Bearer <token>
```

Response:
```json
{ "listenKey": "abc123..." }
```

Keepalive (extend by 60 minutes): `PUT /listen-key`
Invalidate: `DELETE /listen-key`

---

## 2. Rate Limits

Limits are per API key (authenticated) or per IP (unauthenticated). The limiter uses a token-bucket algorithm.

| Tier | Steady-State | Burst |
|------|-------------|-------|
| Unauthenticated | 30 req/sec | 60 |
| Authenticated | 60 req/sec | 120 |
| VIP 3+ | 120 req/sec | 240 |
| Market Maker | 300 req/sec | 600 |

Rate limit status is returned in every response header:

| Header | Meaning |
|--------|---------|
| `X-RateLimit-Limit` | Bucket refill rate (req/sec) |
| `X-RateLimit-Used-Weight` | Total weight consumed this minute |
| `X-RateLimit-Order-Count` | Order placement requests this minute |

When exceeded, the server returns HTTP `429 Too Many Requests`. Back off and retry.

---

## 3. Response Format

Successful responses return HTTP `200` with a JSON body. The body structure is endpoint-specific — there is no wrapper envelope.

Error responses return an appropriate HTTP status code with:

```json
{
  "error": "Human-readable description of the problem"
}
```

Some endpoints return extended error context:

```json
{
  "error": "Insufficient balance",
  "available": "95.00",
  "required": "100.00"
}
```

---

## 4. Top Endpoints

All private endpoints require authentication (JWT or API key).

---

### Market Data (Public)

#### GET /markets

List all tradable symbols with basic info.

```
GET /markets
GET /markets/all         — extended info including contract specs
```

Response (array):
```json
[
  {
    "symbol": "BTCUSDT",
    "base_asset": "BTC",
    "quote_asset": "USDT",
    "product_type": "perpetual",
    "status": "trading",
    "tick_size": "0.1",
    "lot_size": "0.001",
    "min_qty": "0.001",
    "max_leverage": "100"
  }
]
```

---

#### GET /markets/{symbol}/ticker

24-hour rolling ticker for a symbol.

```
GET /markets/BTCUSDT/ticker
```

Response:
```json
{
  "symbol": "BTCUSDT",
  "last_price": "67450.00",
  "last_qty": "0.002",
  "mark_price": "67455.12",
  "index_price": "67448.90",
  "open_24h": "65000.00",
  "high_24h": "68100.00",
  "low_24h": "64800.00",
  "volume_24h": "12345.678",
  "quote_volume_24h": "831234567.89",
  "price_change_pct": "3.77",
  "funding_rate": "0.0001",
  "next_funding_time": 1711929600000
}
```

---

#### GET /markets/{symbol}/orderbook

Order book snapshot. Default depth: 25 levels. Maximum: 1000.

```
GET /markets/BTCUSDT/orderbook?depth=10
```

Response:
```json
{
  "symbol": "BTCUSDT",
  "bids": [["67450.0", "1.234"], ["67449.5", "0.500"]],
  "asks": [["67451.0", "0.800"], ["67451.5", "2.100"]],
  "best_bid": "67450.0",
  "best_ask": "67451.0",
  "spread": "1.0",
  "mid_price": "67450.5"
}
```

Also available:
- `GET /markets/{symbol}/orderbook/l3` — L3 order-by-order book
- `GET /markets/{symbol}/bbo` — best bid/offer only

---

#### GET /markets/{symbol}/klines

OHLCV candlestick data.

```
GET /markets/BTCUSDT/klines?interval=1h&limit=100
```

Query parameters:

| Param | Required | Values |
|-------|----------|--------|
| `interval` | Yes | `1s`, `1m`, `3m`, `5m`, `15m`, `30m`, `1h`, `2h`, `4h`, `6h`, `8h`, `12h`, `1d`, `3d`, `1w`, `1M` |
| `limit` | No | 1–1000, default 200 |
| `start_time` | No | Unix ms |
| `end_time` | No | Unix ms |

Response (array of candles, oldest first):
```json
[
  {
    "open_time": 1711926000000,
    "open": "67000.00",
    "high": "67500.00",
    "low": "66800.00",
    "close": "67450.00",
    "volume": "234.567",
    "close_time": 1711929599999
  }
]
```

Mark-price klines: `GET /markets/{symbol}/mark-klines`
Index-price klines: `GET /markets/{symbol}/index-klines`

---

#### GET /markets/{symbol}/trades

Recent public trades.

```
GET /markets/BTCUSDT/trades?limit=50
```

Response:
```json
[
  {
    "trade_id": "...",
    "price": "67450.00",
    "qty": "0.010",
    "side": "Buy",
    "timestamp": 1711929550000
  }
]
```

---

#### GET /funding/rate

Current and historical funding rates for perpetual contracts.

```
GET /funding/rate?symbol=BTCUSDT
GET /funding/history?symbol=BTCUSDT&limit=100
```

---

### Trading (Private)

#### POST /orders — Place Order

```
POST /orders
Authorization: Bearer <token>

{
  "symbol": "BTCUSDT",
  "side": "buy",
  "order_type": "limit",
  "qty": "0.01",
  "price": "60000.00",
  "time_in_force": "gtc",
  "order_link_id": "my-order-001",
  "take_profit": "65000.00",
  "stop_loss": "57000.00"
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `symbol` | string | Yes | e.g. `"BTCUSDT"` |
| `side` | string | Yes | `"buy"` or `"sell"` |
| `order_type` | string | Yes | `"limit"` or `"market"` |
| `qty` | decimal | Yes | Base quantity |
| `price` | decimal | Limit only | Omit for market orders |
| `time_in_force` | string | No | `"gtc"` (default), `"ioc"`, `"fok"`, `"post_only"`, `"gtd"` |
| `expire_at` | int (ms) | GTD only | Unix ms expiry |
| `reduce_only` | bool | No | Only reduces an open position |
| `close_position` | bool | No | Close full position (ignores qty) |
| `take_profit` | decimal | No | Creates linked TP trigger on fill |
| `stop_loss` | decimal | No | Creates linked SL trigger on fill |
| `iceberg_qty` | decimal | No | Visible portion for iceberg orders |
| `order_link_id` | string | No | Client order ID, max 36 chars |
| `slippage_tolerance` | decimal | Market only | Max slippage %, e.g. `"1.0"` |
| `stp_mode` | string | No | `"expire_taker"`, `"expire_maker"`, `"expire_both"`, `"none"` |
| `price_match` | string | No | `"queue"` or `"opponent"` |
| `position_idx` | int | Hedge mode | `1`=long, `2`=short |

Response:
```json
{
  "order_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "order_link_id": "my-order-001",
  "status": "open"
}
```

---

#### GET /orders — Open Orders

```
GET /orders
GET /orders?symbol=BTCUSDT
```

Response (array):
```json
[
  {
    "order_id": "...",
    "symbol": "BTCUSDT",
    "side": "Buy",
    "order_type": "Limit",
    "price": "60000.00",
    "qty": "0.01",
    "filled_qty": "0.00",
    "status": "Open",
    "time_in_force": "Gtc",
    "created_at": 1711929500000
  }
]
```

---

#### DELETE /orders/{order_id} — Cancel Order

```
DELETE /orders/a1b2c3d4-e5f6-7890-abcd-ef1234567890
```

Response:
```json
{ "order_id": "a1b2c3d4-..." }
```

Cancel by client order ID: `DELETE /orders/by-link-id/{link_id}`

Cancel all: `POST /orders/cancel-all` (optionally filter by symbol)

---

#### PUT /orders/{order_id} — Amend Order

Modify the price and/or quantity of a resting order.

```
PUT /orders/a1b2c3d4-...
{ "price": "61000.00", "qty": "0.02" }
```

---

#### GET /orders/history — Order History

```
GET /orders/history?symbol=BTCUSDT&limit=50&cursor=<next_cursor>
```

---

#### POST /orders/batch — Batch Place

```
POST /orders/batch
{ "orders": [ {...}, {...} ] }
```

Up to 10 orders per batch. Returns an array of results in the same order.

Also: `DELETE /orders/batch` (batch cancel), `PUT /orders/batch-amend` (batch amend).

---

#### POST /trigger-orders — Place Trigger Order

```
POST /trigger-orders
{
  "symbol": "BTCUSDT",
  "side": "sell",
  "trigger_type": "stop_market",
  "trigger_price": "57000.00",
  "qty": "0.01"
}
```

`trigger_type` values: `stop_market`, `stop_limit`, `take_profit_market`, `take_profit_limit`, `trailing_stop`

For trailing stop, use `trailing_delta` (absolute) or `trailing_delta_pct` (percentage) instead of `trigger_price`.

---

#### POST /orders/bracket — Bracket Order

Place entry + TP + SL in one call:

```
POST /orders/bracket
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

---

### Account (Private)

#### GET /account/balances

```
GET /account/balances
```

Response:
```json
[
  {
    "asset": "USDT",
    "total": "10000.00",
    "locked": "600.00",
    "available": "9400.00"
  }
]
```

---

#### GET /account/fee-rate

```
GET /account/fee-rate
```

Response:
```json
{
  "makerFeeRate": "0.001",
  "takerFeeRate": "0.002",
  "vipLevel": "Regular",
  "vipLevelIndex": 0,
  "volume30d": "0.00"
}
```

---

#### GET /account/trades — Trade History

```
GET /account/trades?symbol=BTCUSDT&limit=100
```

---

#### GET /account/wallet — Wallet Overview

Combined balance, margin, and unrealized PnL summary.

```
GET /account/wallet
```

---

#### POST /account/api-keys — Create API Key

```
POST /account/api-keys
{ "label": "my-bot", "permissions": "trading" }
```

Permissions: `read_only`, `trading`, `full`

Response includes the secret — store it securely, it is not shown again.

---

### Positions (Private)

#### GET /positions

```
GET /positions
GET /positions?symbol=BTCUSDT
```

Response:
```json
[
  {
    "symbol": "BTCUSDT",
    "side": "Buy",
    "size": "0.01",
    "entry_price": "60000.00",
    "mark_price": "67450.00",
    "unrealized_pnl": "74.50",
    "realized_pnl": "0.00",
    "margin": "60.00",
    "leverage": "10",
    "margin_mode": "Isolated",
    "liq_price": "54550.00"
  }
]
```

---

#### POST /position/leverage

```
POST /position/leverage
{
  "symbol": "BTCUSDT",
  "buy_leverage": "10",
  "sell_leverage": "10"
}
```

---

#### POST /position/trading-stop

Set or update TP/SL for an open position:

```
POST /position/trading-stop
{
  "symbol": "BTCUSDT",
  "take_profit": "70000.00",
  "stop_loss": "58000.00",
  "position_idx": 0
}
```

---

### Wallet (Private)

#### POST /account/deposit-address

```
POST /account/deposit-address
{ "asset": "USDT", "network": "TRC20" }
```

#### POST /account/withdraw

```
POST /account/withdraw
{
  "asset": "USDT",
  "amount": "500.00",
  "address": "TXyz...",
  "network": "TRC20"
}
```

#### GET /account/deposits / GET /account/withdrawals

Transaction history lists.

---

## 5. WebSocket

Connect: `wss://api.exchange.example/ws/v1`

Multi-stream shortcut (no subscribe message needed):
```
wss://.../ws/v1?stream=trade.BTCUSDT,orderbook.BTCUSDT
```

### Subscribe / Unsubscribe

```json
{ "op": "subscribe", "args": ["trade.BTCUSDT", "kline.1m.BTCUSDT"] }
```

Acknowledgement:
```json
{ "op": "subscribe", "success": true }
```

Data messages:
```json
{ "op": "data", "topic": "trade.BTCUSDT", "data": { ... } }
```

### Public Topics

| Topic | Description |
|-------|-------------|
| `trade.{symbol}` | Every public trade fill |
| `orderbook.{symbol}` | Full snapshot, then incremental diffs |
| `orderbook.diff.{symbol}` | Incremental diff with `seq` / `prevSeq` |
| `orderbook.1.{symbol}` | Best bid/ask only |
| `orderbook.50.{symbol}` | Top-50 levels, refreshed every 100 ms |
| `ticker.{symbol}` | 24h rolling ticker after each trade |
| `ticker.mini.{symbol}` | Lightweight: last price + 24h volume only |
| `ticker.all` | Mini ticker for all symbols |
| `kline.{interval}.{symbol}` | Candle updates (all intervals supported) |
| `liquidation.{symbol}` | Public liquidation events |
| `funding.{symbol}` | Funding rate updates |
| `markPrice.{symbol}` | Real-time mark price + index price |

### Sequence Numbers (Order Book Diffs)

`orderbook.diff.{symbol}` messages carry `seq` and `prevSeq`. If `prevSeq` does not match your last seen `seq`, you have a gap. Re-subscribe to receive a fresh snapshot.

### Private Topics (Authentication Required)

Authenticate after connecting:

```json
{ "op": "auth", "args": ["api_key", "1711929500000", "hex_signature"] }
```

Or with ListenKey:

```json
{ "op": "auth", "listenKey": "abc123..." }
```

| Topic | Description |
|-------|-------------|
| `orders` | Order status changes (placed, partially filled, filled, cancelled) |
| `fills` | Trade fills with fee detail |
| `positions` | Position updates (size, PnL, liquidation price) |
| `wallet` | Balance snapshot updated on any balance change |
| `funding` | Funding payments debited/credited |
| `deposits` | Deposit credits |
| `adl` | Auto-deleverage events |
| `adl.{symbol}` | ADL warning for a specific symbol |

---

## 6. Error Reference

All errors return a JSON body with an `"error"` field and an HTTP status code. There is no numeric error code enum — use the HTTP status and the message string.

| HTTP Status | Meaning | Common Causes |
|-------------|---------|---------------|
| `400 Bad Request` | Invalid request parameters | Missing required field, invalid symbol, price outside tick size |
| `401 Unauthorized` | Not authenticated | Missing or expired JWT, invalid API key |
| `403 Forbidden` | Authenticated but not permitted | API key lacks required permission (e.g., withdraw with read_only key), IP not in allowlist |
| `404 Not Found` | Resource does not exist | Order ID not found, symbol not listed |
| `409 Conflict` | Duplicate request | `order_link_id` already used |
| `422 Unprocessable Entity` | Business logic rejection | Insufficient balance, reduce-only would open a new position, post-only would take liquidity |
| `429 Too Many Requests` | Rate limit exceeded | Slow down and retry after `Retry-After` header |
| `503 Service Unavailable` | System temporarily unavailable | Index price feed unhealthy (perpetuals only), matching engine not ready |

### Common Error Messages

| Message | Explanation |
|---------|-------------|
| `"Insufficient balance"` | Available balance too low for the order |
| `"Invalid symbol"` | Symbol does not exist or is not tradable |
| `"Post-only order would take liquidity"` | Use `gtc` or `ioc` instead, or adjust price |
| `"Reduce-only order would open position"` | Order side does not reduce the open position |
| `"Index price unavailable"` | Perpetual order rejected — index feed unhealthy |
| `"KYC not verified"` | Account requires Basic KYC before trading |
| `"Timestamp expired"` | API key signature is too old (check system clock) |
| `"Authentication required"` | JWT or API key missing from the request |
| `"API key lacks the required permission"` | Key does not have trade or withdraw permission |
| `"Account not found"` | User has no funded account yet |

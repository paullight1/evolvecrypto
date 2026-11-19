# Trading UX Marathon — Make It a Real Exchange

## Wave T1: Spot vs Perpetual Distinction

### T1.1 Product Type System
- [ ] Frontend: `ProductType` enum: 'spot' | 'perpetual'
- [ ] PairList: show type badge (SPOT/PERP) per pair
- [ ] TickerBar: show product type, different info for spot vs perp
- [ ] OrderEntry: hide leverage/margin for spot, show for perp
- [ ] Spot: no funding rate, no mark price, no liquidation
- [ ] Perp: show funding rate, mark price, index price, open interest

### T1.2 Spot Trading Mode
- [ ] No leverage slider (always 1x)
- [ ] No margin mode toggle
- [ ] No TP/SL on position (spot has no "position")
- [ ] Buy = spend quote, receive base. Sell = spend base, receive quote
- [ ] Balance shows: available base + available quote
- [ ] No position panel for spot — just order history + balances

### T1.3 Perpetual Trading Mode
- [ ] Leverage slider (1x-100x configurable per symbol)
- [ ] Cross / Isolated margin toggle (functional, not just UI)
- [ ] Margin display: initial margin, maintenance margin
- [ ] Funding rate with countdown timer (next settlement)
- [ ] Mark Price / Last Price / Index Price all visible
- [ ] Open Interest display
- [ ] Position panel with full details

## Wave T2: Real-Time Data Streams

### T2.1 Ticker WebSocket Stream
- [ ] Subscribe to `ticker.{symbol}` on symbol change
- [ ] Update ALL ticker fields in real-time:
  - Last price (with color flash green/red)
  - 24h change %
  - 24h high / low
  - 24h volume (base + quote)
  - Mark price (perp only)
  - Index price (perp only)
  - Funding rate + next funding time (perp only)
  - Open interest (perp only)
- [ ] Unsubscribe from old symbol on switch

### T2.2 Kline/Candle WebSocket Stream
- [ ] Subscribe to `kline.{interval}.{symbol}`
- [ ] Update current (open) candle in real-time
- [ ] New candle on interval close
- [ ] Chart auto-scrolls to latest candle
- [ ] Volume bars update with candle

### T2.3 Orderbook WebSocket
- [ ] Snapshot on subscribe → full book
- [ ] Incremental deltas with sequence numbers
- [ ] Gap detection → resubscribe
- [ ] Throttle render to 10fps max
- [ ] Flash new/changed price levels

### T2.4 Trade Stream
- [ ] Subscribe to `trade.{symbol}`
- [ ] New trades appear at top of recent trades
- [ ] Color by aggressor side (buy=green, sell=red)
- [ ] Sound option for large trades (future)

## Wave T3: Position & Order Management

### T3.1 Position Panel (Perp)
- [ ] Live unrealized PnL (updates with mark price tick)
- [ ] ROE% (return on equity)
- [ ] Margin ratio bar (green → yellow → red as approaching liq)
- [ ] ADL indicator (1-5 dots, from API adl_quantile)
- [ ] Liquidation price (displayed + line on chart)
- [ ] Entry price line on chart
- [ ] TP/SL status with prices
- [ ] Close position: market close, limit close
- [ ] Add margin button (isolated mode)
- [ ] Adjust leverage button
- [ ] Share PnL button → PnL card

### T3.2 Position Lines on Chart
- [ ] Entry price: dashed blue line with label
- [ ] Liquidation price: dashed red line with label
- [ ] TP price: dashed green line (if set)
- [ ] SL price: dashed red line (if set)
- [ ] Lines update when position changes

### T3.3 Order Management
- [ ] Open orders: amend price/qty inline (click to edit)
- [ ] Conditional orders tab (stop/TP/SL triggers)
- [ ] OCO pairs shown as linked
- [ ] Cancel all: per symbol or all symbols
- [ ] Order confirmation modal (can disable with one-click mode)

### T3.4 Wallet Separation
- [ ] Spot Wallet: assets for spot trading
- [ ] Derivatives Wallet: USDT margin for futures
- [ ] Transfer button between wallets (instant, zero fee)
- [ ] Each wallet shows independent balances
- [ ] Trading form shows correct wallet balance

## Wave T4: Advanced Chart Features

### T4.1 Chart Overlays
- [ ] Position entry price line
- [ ] Liquidation price line
- [ ] TP/SL price lines
- [ ] Open order price lines (with cancel button on hover)
- [ ] Mark price line (for perp, distinct from last price candles)

### T4.2 Chart Indicators (via TradingView)
- [ ] Volume bars below candles (already partial)
- [ ] Moving averages (MA, EMA)
- [ ] Bollinger Bands
- [ ] RSI
- [ ] MACD
- [ ] Custom indicator list

### T4.3 Chart Tools
- [ ] Drawing tools: trend line, horizontal line, fib retracement
- [ ] Text annotations
- [ ] Price alerts (click to set, show notification when hit)
- [ ] Screenshot/share chart

### T4.4 Multi-Chart
- [ ] 1x1 (default), 2x1, 2x2, 1x2 layouts
- [ ] Each chart can show different symbol/timeframe
- [ ] Synced crosshair across charts (optional)

## Wave T5: Market Data Pages

### T5.1 Markets Page Enhancement
- [ ] Tabs: Spot | Perpetual | Favorites
- [ ] Per pair: mini chart (sparkline), 24h volume, funding rate (perp)
- [ ] Sort by: volume, change%, name, price
- [ ] Hot pairs: highest volume or biggest movers
- [ ] New listings section

### T5.2 Ticker Tape
- [ ] Scrolling ticker at top of trading page
- [ ] Show top 10 pairs by volume with price + change
- [ ] Click ticker → switch to that pair

### T5.3 Market Stats Widget
- [ ] Total exchange volume (24h)
- [ ] Total open interest
- [ ] BTC dominance
- [ ] Funding rate summary (which pairs positive/negative)

## Wave T6: Trading Features

### T6.1 One-Click Trading
- [ ] Toggle in order entry: ⚡ icon
- [ ] When enabled: no confirmation modal
- [ ] Keyboard shortcuts: B = buy, S = sell, Esc = cancel
- [ ] Visual feedback: button flash on execution

### T6.2 Calculated Fields in Order Form
- [ ] Est. cost (qty * price for limit)
- [ ] Est. margin required (perp: cost / leverage)
- [ ] Est. liquidation price (before placing order)
- [ ] Est. PnL at TP price
- [ ] Est. loss at SL price
- [ ] Max qty buyable with available balance

### T6.3 Price Alerts
- [ ] Set alert: "Notify me when BTC > $55,000"
- [ ] Alert types: above, below, crossing
- [ ] Notification when triggered
- [ ] Manage alerts page
- [ ] Show alert lines on chart

## Estimated Effort

| Wave | Focus | Days |
|------|-------|------|
| T1 | Spot vs Perp | 3-4 |
| T2 | Real-Time Streams | 3-4 |
| T3 | Position & Order Mgmt | 4-5 |
| T4 | Advanced Chart | 3-4 |
| T5 | Market Data Pages | 2-3 |
| T6 | Trading Features | 3-4 |
| **Total** | | **18-24** |

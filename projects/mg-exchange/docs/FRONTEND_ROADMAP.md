# Frontend Roadmap — From Terminal to Full Platform

## Sprint 1: Portal & Auth (Landing → Login → Trading)

### 1.1 Landing Page
- Hero section: headline + CTA "Start Trading"
- Live market ticker scroll (BTC, ETH, SOL prices)
- Market overview table: top pairs with price, 24h%, volume, sparkline
- Feature highlights: low fees, fast execution, security, PoR
- Stats: total volume, users, uptime
- Footer: docs, API, legal, social links

### 1.2 Auth Pages
- Register: email + password + referral code (optional)
- Login: email + password → 2FA if enabled
- Password reset flow (email)
- Post-login redirect to trading terminal

### 1.3 Market Overview Page
- All pairs in filterable/sortable table
- Categories: Perpetual, Spot, New, Hot
- Search, favorites, 7d sparkline charts
- Click → opens trading terminal with that pair

## Sprint 2: Trading Terminal Enhancements

### 2.1 Depth Chart
- Area chart showing bid/ask liquidity walls
- Toggle between orderbook and depth chart view
- Hover shows price + cumulative volume
- Canvas-based for performance

### 2.2 PnL Sharing Cards
- Screenshot position as branded card
- Shows: symbol, side, entry, exit, ROE%, PnL
- Exchange logo + branding
- Copy to clipboard or download as PNG
- Share button on position panel

### 2.3 Command Palette (Cmd+K)
- Search markets, navigate pages, quick actions
- Recent symbols, favorites
- Keyboard shortcuts list

### 2.4 Notification Center
- Bell icon in header with unread count
- Dropdown: order fills, liquidation warnings, funding, system
- Toast notifications for real-time events
- Click notification → navigate to relevant page

## Sprint 3: Business Features UI

### 3.1 Copy Trading Page
- Leaderboard: top traders sorted by PnL, win rate
- Trader profile: stats, equity curve, open positions
- Follow button with allocation input
- My following: list masters, PnL per master, unfollow

### 3.2 Trading Bots UI
- Bot marketplace: Grid, DCA, Rebalancer
- Grid bot config: symbol, upper/lower price, grid count, investment
- DCA bot config: symbol, amount, interval, price deviation
- My bots: running bots with live PnL, stop button

### 3.3 KYC Onboarding
- Step 1: Choose provider (Diia for UA, Sumsub for international)
- Step 2: Diia → QR code display, Sumsub → embedded WebSDK
- Step 3: Status tracking (pending, verified, rejected)
- Post-KYC: limits upgrade notification

## Sprint 4: Polish & Advanced

### 4.1 Mobile Responsive
- Responsive breakpoints: desktop (>1200), tablet (768-1200), mobile (<768)
- Mobile: stacked layout, swipeable panels
- Bottom tab bar for mobile navigation

### 4.2 Themes & Customization
- Light theme option
- Custom accent color
- Save layout preferences to localStorage

### 4.3 Advanced Chart Features
- Drawing tools toolbar
- Indicator list with search
- Multi-chart layout (2x2 grid)
- Price alerts on chart (horizontal line)

### 4.4 Proof of Reserves Page
- Public page showing latest PoR report
- Verify your balance: enter user ID → see Merkle proof
- Asset breakdown with reserve ratios
- Historical PoR reports

# Frontend Marathon — From MVP to Production Platform

## Current State After Sprint 1-4

Done: Landing, Auth, Markets, Trading Terminal (rewritten), Depth Chart, PnL Cards,
Cmd+K, Notifications, Copy Trading, Bots, KYC, PoR, Admin Panel, Mobile Responsive.

## Sprint 5: Wallet & Financial Pages

### 5.1 Wallet Dashboard
- Portfolio overview: total equity in USD, asset breakdown pie chart
- Per-asset rows: icon, name, total, available, locked, USD value
- Quick actions: Deposit, Withdraw, Transfer per asset
- Portfolio equity curve (7d/30d/90d line chart)

### 5.2 Deposit Page (Enhanced)
- Network selector with fee + confirmation time per network
- QR code (generate from address string — use qrcode lib or canvas)
- Copy address button with visual feedback
- Deposit history below with status badges (Pending/Confirming/Credited)
- Minimum deposit warning per asset

### 5.3 Withdrawal Page (Enhanced)
- Address book (saved addresses from whitelist)
- Network fee display + "You will receive" calculator
- 2FA code input (if enabled)
- Daily limit progress bar (used/remaining)
- Withdrawal history with tx hash links to block explorer

### 5.4 Transfer Page
- Sub-account selector (from → to)
- Asset + amount
- Instant, zero fee
- Transfer history

## Sprint 6: Referrals & Social

### 6.1 Referral Dashboard
- My referral code (copy button, share link)
- Stats: total referred, active traders, total earned
- Commission history table (date, trader, trade, commission)
- Tier info: Level 1 (20%) + Level 2 (5%)
- Invite friends: email input, social share buttons

### 6.2 PnL Social Feed (OKX Orbit inspired)
- Public feed of verified PnL cards
- Users opt-in to share
- Like / comment (future)
- Filter by: symbol, timeframe, top PnL

### 6.3 Leaderboard Page
- Overall top traders (30d PnL, ROE, win rate)
- Per-symbol leaderboards
- My ranking badge

## Sprint 7: Settings & Preferences

### 7.1 Settings Page
- Notification preferences (toggle per type: fills, warnings, funding, login)
- Display preferences: default leverage, default order type, chart theme
- Language selector (UA / EN)
- Timezone selector
- Default trading pair

### 7.2 Internationalization (i18n)
- Extract all UI strings to translation files
- Ukrainian (default) + English
- Number formatting per locale (1,234.56 vs 1 234,56)
- Date formatting per locale

### 7.3 Theme System
- Dark (default) / Light toggle
- Custom accent color picker
- Save to localStorage

## Sprint 8: Documentation & Help

### 8.1 API Documentation Page
- Interactive API explorer (like Swagger but styled)
- WebSocket protocol docs with examples
- Code examples: Python, JavaScript, curl
- Rate limit info per endpoint
- Authentication guide

### 8.2 Help Center
- FAQ accordion
- Categories: Trading, Account, Deposits, Withdrawals, Fees, Security
- Search
- Contact support form

### 8.3 Trading Guide
- For beginners: what is spot/futures, how to place order
- Interactive: tooltips on terminal elements explaining each panel
- Video placeholders

## Sprint 9: Performance & Polish

### 9.1 Code Splitting
- React.lazy for all pages (landing, markets, account, etc.)
- Suspense with loading fallbacks
- Preload critical routes

### 9.2 Performance Optimization
- React.memo on OrderBook rows, Position rows, Order rows
- useMemo for computed values (depth bars, PnL calculations)
- useCallback for all handlers passed as props
- Web Worker for orderbook delta processing
- requestAnimationFrame throttle on all real-time updates
- Bundle analysis: keep initial load < 200KB gzipped

### 9.3 Error Handling
- React Error Boundary per panel (chart crash doesn't kill terminal)
- Global error boundary with "Something went wrong, reload?" UI
- API error toast notifications (not silent failures)
- WebSocket disconnect overlay: "Reconnecting..." with retry countdown

### 9.4 Empty States & Onboarding
- First visit: subtle tutorial overlay (dismiss forever)
- No positions: "Open your first position" CTA
- No orders: "Place your first order" hint
- No deposits: "Fund your account to start trading"
- No KYC: "Complete verification to unlock full limits" banner

### 9.5 Animations & Micro-interactions
- Page transitions (fade in/out)
- Order placed: brief green/red flash on the button
- Position PnL: smooth number counter animation
- Orderbook: row highlight on update (subtle flash)
- Toast slide-in/out
- Modal backdrop fade
- Button hover/active states
- Loading skeleton shimmer everywhere

## Sprint 10: PWA & Advanced

### 10.1 PWA Support
- Service worker for offline caching
- App manifest for "Add to Home Screen"
- Offline indicator
- Cache strategy: network-first for API, cache-first for static

### 10.2 Advanced Chart Features
- Drawing tools toolbar (trend line, horizontal line, fib retracement)
- Indicator list with search and add
- Multi-chart layout: 1x1, 2x1, 2x2
- Chart templates (save/load indicator setups)
- Price alerts: click to set horizontal alert line

### 10.3 Status Page
- System health for users (not admin)
- Service status: Trading ✅, Deposits ✅, Withdrawals ⚠️
- Incident history
- Subscribe to status updates

### 10.4 SEO & Meta
- Landing page: proper meta tags, OG images
- Dynamic title per page
- Structured data (JSON-LD for organization)
- Sitemap.xml
- robots.txt

## Priority Order

Sprint 5: Wallet (CRITICAL — users need deposit/withdraw)
Sprint 6: Referrals (GROWTH — user acquisition)
Sprint 7: Settings + i18n (QUALITY — user retention)
Sprint 8: Docs + Help (SUPPORT — reduce tickets)
Sprint 9: Performance + Polish (PROFESSIONAL — trust)
Sprint 10: PWA + Advanced (COMPETITIVE — differentiation)

## Estimated Effort

| Sprint | Days | Complexity |
|--------|------|-----------|
| 5 | 5-7 | Medium |
| 6 | 4-5 | Medium |
| 7 | 5-7 | Medium (i18n is tedious) |
| 8 | 4-5 | Low-Medium |
| 9 | 5-7 | High (performance tuning) |
| 10 | 5-7 | High (PWA, advanced charts) |
| **Total** | **28-38** | |

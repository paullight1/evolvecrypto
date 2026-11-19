# UI Redesign — Bybit/Binance Level Quality

## Current Problems
- Looks like 2015, not 2026
- No glassmorphism, no depth, no visual hierarchy
- Orderbook is flat and lifeless
- Chart is basic Lightweight Charts (no indicators, no drawings)
- Order form looks like a simple HTML form
- No proper animations, transitions, micro-interactions
- Typography is inconsistent
- Spacing/padding is off
- Colors don't have proper hierarchy (everything same weight)
- No proper loading states (skeleton vs spinner mix)
- Desktop-first, mobile is broken

## Target: Bybit Pro Terminal (2026)

### Design Principles
1. Dark theme with depth (multiple bg layers, not flat)
2. Numbers are king — mono font, proper alignment, color-coded
3. Less chrome, more data — minimal borders, use shadow/depth instead
4. Motion is subtle — 150ms transitions, no flashy animations
5. Information hierarchy — important data is larger/brighter, secondary is muted
6. Touch-ready — 44px minimum targets, proper spacing

### Color System (refined)
```css
/* Backgrounds — 5 layers of depth */
--bg-0: #06080d;     /* deepest — page bg */
--bg-1: #0c0e14;     /* primary panels */
--bg-2: #12141c;     /* elevated panels, cards */
--bg-3: #181a24;     /* hover states, active tabs */
--bg-4: #1e2030;     /* tooltips, dropdowns */

/* Borders — subtle, not harsh */
--border-0: rgba(255,255,255,0.04);   /* panel dividers */
--border-1: rgba(255,255,255,0.08);   /* input borders */
--border-2: rgba(255,255,255,0.12);   /* active/focus borders */

/* Text — 4 levels */
--text-0: #f0f2f5;   /* primary — prices, headings */
--text-1: #a0a4b0;   /* secondary — labels, descriptions */
--text-2: #5c6070;   /* tertiary — timestamps, hints */
--text-3: #363840;   /* disabled */

/* Trading colors — vibrant but not neon */
--long: #22c55e;     /* buy/long/positive */
--long-bg: rgba(34,197,94,0.08);
--long-hover: #16a34a;
--short: #ef4444;    /* sell/short/negative */
--short-bg: rgba(239,68,68,0.08);
--short-hover: #dc2626;

/* Accent — brand */
--accent: #eab308;   /* gold */
--accent-bg: rgba(234,179,8,0.08);
```

### Typography
```css
--font-sans: 'Inter', -apple-system, system-ui, sans-serif;
--font-mono: 'JetBrains Mono', 'SF Mono', monospace;

/* Scale */
--text-2xs: 10px;    /* timestamps, minimal labels */
--text-xs: 11px;     /* table headers, small labels */
--text-sm: 12px;     /* table data, secondary info */
--text-base: 13px;   /* default body */
--text-md: 14px;     /* prices in orderbook */
--text-lg: 16px;     /* section titles */
--text-xl: 20px;     /* ticker price */
--text-2xl: 28px;    /* hero price on landing */
```

### Component Redesign List

1. **TickerBar** — thinner (36px), proper stat spacing, subtle bg gradient
2. **OrderBook** — depth gradient bars (not solid), animated qty changes, proper spread display
3. **OrderEntry** — card-style sections, better leverage UI, cleaner TP/SL
4. **Chart** — dark bg matching panels, proper toolbar
5. **BottomPanel** — tabs with counts, proper table styling
6. **PairList** — compact rows, sparklines, proper hover
7. **RecentTrades** — flash animation on new trades
8. **Modals** — glassmorphism backdrop, rounded corners, shadows
9. **Buttons** — gradient fills for primary, proper hover/active states
10. **Inputs** — darker bg than panel, subtle borders, focus glow
11. **Tables** — alternating rows, hover highlight, proper column alignment
12. **Badges** — rounded, subtle bg, proper sizing

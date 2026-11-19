import { useState, useEffect, useCallback } from 'react';
import { navigate } from '@app/Router';
import { getTicker } from '@features/trading/api';
import type { Ticker } from '@features/trading/types';
import styles from './LandingPage.module.css';

const TICKER_SYMBOLS = [
  'BTCUSDT', 'ETHUSDT', 'SOLUSDT', 'BNBUSDT',
  'XRPUSDT', 'DOGEUSDT', 'AVAXUSDT', 'ADAUSDT',
];

// ── SVG icon components ──────────────────────────────────────────────────────

function IconLightning() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M13 2L4.5 13.5H11L10 22L20.5 10.5H14L13 2Z" fill="#2ee5a6" />
    </svg>
  );
}

function IconShield() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M12 2L3 6V12C3 16.97 7.02 21.61 12 22C16.98 21.61 21 16.97 21 12V6L12 2Z" fill="#2ee5a6" />
      <path d="M9 12L11 14L15 10" stroke="#0b0e11" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function IconCode() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M8 6L2 12L8 18" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M16 6L22 12L16 18" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M14 4L10 20" stroke="#2ee5a6" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function IconGlobe() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <circle cx="12" cy="12" r="9" stroke="#2ee5a6" strokeWidth="2" />
      <path d="M12 3C12 3 8 7 8 12C8 17 12 21 12 21" stroke="#2ee5a6" strokeWidth="1.5" />
      <path d="M12 3C12 3 16 7 16 12C16 17 12 21 12 21" stroke="#2ee5a6" strokeWidth="1.5" />
      <path d="M3 12H21" stroke="#2ee5a6" strokeWidth="1.5" />
    </svg>
  );
}

function IconChartLine() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <polyline points="3,18 8,11 13,14 21,5" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" fill="none" />
      <line x1="3" y1="18" x2="21" y2="18" stroke="#2ee5a6" strokeWidth="1" opacity="0.4" />
    </svg>
  );
}

function IconArrowsCycle() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M4 12C4 7.58 7.58 4 12 4C14.5 4 16.74 5.12 18.25 6.9L16 9H22V3L19.77 5.23C17.84 3.24 15.07 2 12 2C6.48 2 2 6.48 2 12H4Z" fill="#2ee5a6" />
      <path d="M20 12C20 16.42 16.42 20 12 20C9.5 20 7.26 18.88 5.75 17.1L8 15H2V21L4.23 18.77C6.16 20.76 8.93 22 12 22C17.52 22 22 17.52 22 12H20Z" fill="#2ee5a6" />
    </svg>
  );
}

function IconTarget() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <circle cx="12" cy="12" r="9" stroke="#2ee5a6" strokeWidth="2" />
      <circle cx="12" cy="12" r="5" stroke="#2ee5a6" strokeWidth="2" />
      <circle cx="12" cy="12" r="2" fill="#2ee5a6" />
      <line x1="12" y1="2" x2="12" y2="5" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
      <line x1="12" y1="19" x2="12" y2="22" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
      <line x1="2" y1="12" x2="5" y2="12" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
      <line x1="19" y1="12" x2="22" y2="12" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

function IconCoins() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <ellipse cx="14" cy="8" rx="6" ry="3" fill="#2ee5a6" opacity="0.9" />
      <path d="M8 8V11C8 12.66 10.69 14 14 14C17.31 14 20 12.66 20 11V8" stroke="#2ee5a6" strokeWidth="1.5" />
      <ellipse cx="10" cy="14" rx="6" ry="3" fill="#2ee5a6" opacity="0.7" />
      <path d="M4 14V17C4 18.66 6.69 20 10 20C13.31 20 16 18.66 16 17V14" stroke="#2ee5a6" strokeWidth="1.5" />
    </svg>
  );
}

function IconPeople() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <circle cx="9" cy="7" r="3" fill="#2ee5a6" />
      <path d="M3 20C3 17 5.69 15 9 15C12.31 15 15 17 15 20H3Z" fill="#2ee5a6" />
      <circle cx="16" cy="7" r="2.5" fill="#2ee5a6" opacity="0.7" />
      <path d="M15.5 15C16.62 15 18.5 15.3 20 16.5C21 17.3 21.5 18.1 21.5 20H16" stroke="#2ee5a6" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function IconRobot() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <rect x="5" y="8" width="14" height="10" rx="3" stroke="#2ee5a6" strokeWidth="2" />
      <rect x="9" y="11" width="2" height="2" fill="#2ee5a6" />
      <rect x="13" y="11" width="2" height="2" fill="#2ee5a6" />
      <path d="M9 15.5H15" stroke="#2ee5a6" strokeWidth="1.5" strokeLinecap="round" />
      <line x1="12" y1="8" x2="12" y2="5" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
      <circle cx="12" cy="4" r="1.5" fill="#2ee5a6" />
      <line x1="2" y1="12" x2="5" y2="12" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
      <line x1="19" y1="12" x2="22" y2="12" stroke="#2ee5a6" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

// Social SVG icons
function IconTwitter() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
    </svg>
  );
}

function IconTelegram() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
    </svg>
  );
}

function IconDiscord() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028c.462-.63.874-1.295 1.226-1.994a.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.956 2.418-2.157 2.418zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.947 2.418-2.157 2.418z"/>
    </svg>
  );
}

function IconGitHub() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"/>
    </svg>
  );
}

// ── Icon container wrapper ────────────────────────────────────────────────────

function IconBox({ children }: { children: React.ReactNode }) {
  return (
    <div className={styles.iconBox} aria-hidden="true">
      {children}
    </div>
  );
}

// ── Data ─────────────────────────────────────────────────────────────────────

const FEATURES = [
  {
    icon: <IconLightning />,
    title: 'Sub-Millisecond Matching',
    desc: 'Price-time FIFO matching engine written in Rust. Orders execute in under 500 microseconds with zero GC pauses.',
  },
  {
    icon: <IconShield />,
    title: 'Proof of Reserves',
    desc: 'Cryptographic Merkle tree proofs published monthly. Verify your balance is 100% backed at any time.',
  },
  {
    icon: <IconCode />,
    title: 'Open-Source Engine',
    desc: 'The matching engine and order gateway are open source. Audit the code that handles your funds.',
  },
  {
    icon: <IconGlobe />,
    title: 'Built in Ukraine',
    desc: "Ukraine's first transparent crypto exchange. VASP registered, locally operated, globally accessible.",
  },
];

const PRODUCTS = [
  {
    icon: <IconChartLine />,
    title: 'Spot Trading',
    desc: '200+ trading pairs with deep liquidity. Maker 0.01% / Taker 0.06%.',
    path: 'trade',
  },
  {
    icon: <IconArrowsCycle />,
    title: 'Perpetual Futures',
    desc: 'Linear and inverse perpetuals with up to 100x leverage and isolated margin.',
    path: 'trade',
  },
  {
    icon: <IconTarget />,
    title: 'Options',
    desc: 'European-style options on BTC and ETH. Trade volatility with defined risk.',
    path: 'options',
  },
  {
    icon: <IconCoins />,
    title: 'Earn',
    desc: 'Flexible savings, locked staking, and liquidity mining. Put idle assets to work.',
    path: 'earn',
  },
  {
    icon: <IconPeople />,
    title: 'Copy Trading',
    desc: 'Follow top traders automatically. Full position mirroring with configurable risk limits.',
    path: 'copy',
  },
  {
    icon: <IconRobot />,
    title: 'Trading Bots',
    desc: 'Grid, DCA, and rebalancer bots. Automate your strategy with no code required.',
    path: 'bots',
  },
];

const STATS = [
  { value: '$1B+', label: '24h Volume' },
  { value: '1,000+', label: 'Active Traders' },
  { value: '200+', label: 'Trading Pairs' },
  { value: '<0.5ms', label: 'Avg Latency' },
];

const FOOTER_COLUMNS = [
  {
    title: 'Products',
    links: [
      { label: 'Spot Trading', path: 'trade' },
      { label: 'Perpetual Futures', path: 'trade' },
      { label: 'Options', path: 'options' },
      { label: 'Margin Trading', path: 'margin' },
      { label: 'Earn', path: 'earn' },
    ],
  },
  {
    title: 'Services',
    links: [
      { label: 'Trading Bots', path: 'bots' },
      { label: 'Copy Trading', path: 'copy' },
      { label: 'Algo Execution', path: 'algo' },
      { label: 'Block Trading', path: 'block-trade' },
      { label: 'Convert', path: 'convert' },
    ],
  },
  {
    title: 'Support',
    links: [
      { label: 'Help Center', path: 'help' },
      { label: 'API Docs', path: 'api-docs' },
      { label: 'System Status', path: 'status' },
      { label: 'Fees', path: 'fees' },
      { label: 'Announcements', path: 'social' },
    ],
  },
  {
    title: 'Resources',
    links: [
      { label: 'Markets', path: 'markets' },
      { label: 'Proof of Reserves', path: 'proof-of-reserves' },
      { label: 'Insurance Fund', path: 'insurance-fund' },
      { label: 'Launchpad', path: 'launchpad' },
      { label: 'Competitions', path: 'competitions' },
    ],
  },
  {
    title: 'Legal',
    links: [
      { label: 'Terms of Service', path: 'legal' },
      { label: 'Privacy Policy', path: 'legal' },
      { label: 'AML Policy', path: 'legal' },
      { label: 'Risk Warning', path: 'legal' },
      { label: 'Cookie Policy', path: 'legal' },
    ],
  },
  {
    title: 'Community',
    links: [
      { label: 'Referral Program', path: 'referral' },
      { label: 'Achievements', path: 'achievements' },
      { label: 'Rewards Hub', path: 'rewards' },
      { label: 'Leaderboard', path: 'competitions' },
      { label: 'Trader Profiles', path: 'trader-profile' },
    ],
  },
];

// Static volume bar heights — deterministic, no Math.random()
const VOLUME_HEIGHTS = [22, 14, 18, 11, 24, 16, 20, 13, 19, 15, 17, 21, 12, 23, 16];

// Static order book qty values — deterministic
const ASK_QTYS = ['0.124', '0.084', '0.213', '0.056', '0.178'];
const BID_QTYS = ['0.318', '0.092', '0.145', '0.067', '0.203'];
const ASK_WIDTHS = [80, 95, 72, 88, 100];
const BID_WIDTHS = [92, 78, 85, 70, 97];

// ── Helpers ───────────────────────────────────────────────────────────────────

function formatPrice(price: string): string {
  const n = parseFloat(price);
  if (n >= 10000) return n.toLocaleString('en-US', { maximumFractionDigits: 0 });
  if (n >= 1) return n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  return n.toLocaleString('en-US', { minimumFractionDigits: 4, maximumFractionDigits: 4 });
}

function stripQuote(symbol: string): string {
  return symbol.replace(/USDT$/, '').replace(/USD$/, '');
}

// ── Hooks ─────────────────────────────────────────────────────────────────────

function useScrollReveal() {
  useEffect(() => {
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver((entries) => {
      entries.forEach((e) => {
        if (e.isIntersecting) {
          e.target.classList.add('revealed');
          observer.unobserve(e.target);
        }
      });
    }, { threshold: 0.1 });
    document.querySelectorAll('[data-reveal]').forEach((el) => observer.observe(el));
    return () => observer.disconnect();
  }, []);
}

// ── Main component ────────────────────────────────────────────────────────────

export default function LandingPage() {
  const [tickers, setTickers] = useState<Ticker[]>([]);
  const [tickerLoading, setTickerLoading] = useState(true);
  const [bannerDismissed, setBannerDismissed] = useState<boolean>(() => {
    try {
      return localStorage.getItem('banner-options-live') === 'dismissed';
    } catch {
      return false;
    }
  });

  useScrollReveal();

  useEffect(() => {
    let cancelled = false;
    Promise.all(TICKER_SYMBOLS.map((s) => getTicker(s)))
      .then((results) => {
        if (!cancelled) {
          setTickers(results);
          setTickerLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setTickers([]);
          setTickerLoading(false);
        }
      });
    return () => { cancelled = true; };
  }, []);

  const dismissBanner = useCallback(() => {
    try {
      localStorage.setItem('banner-options-live', 'dismissed');
    } catch {
      // ignore
    }
    setBannerDismissed(true);
  }, []);

  // Find BTC and ETH tickers for live price display
  const btcTicker = tickers.find((t) => t.symbol === 'BTCUSDT');
  const ethTicker = tickers.find((t) => t.symbol === 'ETHUSDT');

  return (
    <div className={styles.page}>
      {/* ── Announcement Banner ── */}
      {!bannerDismissed && (
        <div className={styles.announcementBanner} role="banner">
          <span className={styles.bannerText}>
            Options Trading Now Live — Trade BTC &amp; ETH calls and puts
            <button
              className={styles.bannerCta}
              onClick={() => navigate('options')}
              type="button"
            >
              Explore →
            </button>
          </span>
          <button
            className={styles.bannerClose}
            onClick={dismissBanner}
            type="button"
            aria-label="Dismiss announcement"
          >
            ✕
          </button>
        </div>
      )}

      {/* ── Header ── */}
      <header className={styles.header}>
        <button
          className={styles.headerLogo}
          onClick={() => navigate('')}
          type="button"
          aria-label="Exchange home"
        >
          Exchange
        </button>

        <nav className={styles.headerNav} aria-label="Landing navigation">
          <button className={styles.headerNavLink} onClick={() => navigate('trade')} type="button">
            Trade
          </button>
          <button className={styles.headerNavLink} onClick={() => navigate('markets')} type="button">
            Markets
          </button>
          <button className={styles.headerNavLink} onClick={() => navigate('earn')} type="button">
            Earn
          </button>
          <button className={styles.headerNavLink} onClick={() => navigate('copy')} type="button">
            Copy Trading
          </button>
          <button className={styles.headerNavLink} onClick={() => navigate('bots')} type="button">
            Bots
          </button>
        </nav>

        <div className={styles.headerActions}>
          <button className={styles.btnLogin} onClick={() => navigate('auth')} type="button">
            Log In
          </button>
          <button className={styles.btnRegister} onClick={() => navigate('auth')} type="button">
            Register
          </button>
        </div>
      </header>

      {/* ── Hero ── */}
      <section className={styles.hero} aria-label="Hero">
        <div className={styles.heroLeft}>
          <h1 className={styles.heroHeadline}>
            Trade Crypto with{' '}
            <span className={styles.heroAccent}>Confidence</span>
          </h1>
          <p className={styles.heroSubline}>
            The Exchange You Can <span className={styles.heroAccent}>Audit</span>
          </p>
          <p className={styles.heroSubtext}>
            Institutional-grade matching engine with cryptographically verified reserves.
            Open source, auditable, built in Ukraine. transparent exchange delivering
            sub-millisecond execution.
          </p>
          <p className={styles.heroPillRow}>
            Open-source matching engine.{' '}
            Cryptographic proof of reserves.{' '}
            Sub-millisecond execution.
          </p>
          <div className={styles.heroCtas}>
            <button className={styles.ctaPrimary} onClick={() => navigate('auth')} type="button">
              Get Started
            </button>
            <button className={styles.ctaSecondary} onClick={() => navigate('trade')} type="button">
              Try Demo
            </button>
          </div>
          <div className={styles.trustBadges}>
            <span className={styles.trustBadge}>VASP Registered</span>
            <span className={styles.trustBadge}>100% Reserves</span>
            <span className={styles.trustBadge}>&lt;0.5ms Latency</span>
          </div>

          {/* Live price display */}
          {(btcTicker != null || ethTicker != null) && (
            <div className={styles.livePrices}>
              {btcTicker != null && (() => {
                const pct = parseFloat(btcTicker.price_change_pct_24h);
                const isUp = pct >= 0;
                return (
                  <span className={styles.livePrice}>
                    <span className={styles.livePriceSymbol}>BTC</span>
                    {' '}
                    <span className={styles.livePriceValue}>${formatPrice(btcTicker.last_price)}</span>
                    {' '}
                    <span className={isUp ? styles.livePriceUp : styles.livePriceDown}>
                      {isUp ? '↑' : '↓'} {isUp ? '+' : ''}{pct.toFixed(1)}%
                    </span>
                  </span>
                );
              })()}
              {btcTicker != null && ethTicker != null && (
                <span className={styles.livePriceSep}>|</span>
              )}
              {ethTicker != null && (() => {
                const pct = parseFloat(ethTicker.price_change_pct_24h);
                const isUp = pct >= 0;
                return (
                  <span className={styles.livePrice}>
                    <span className={styles.livePriceSymbol}>ETH</span>
                    {' '}
                    <span className={styles.livePriceValue}>${formatPrice(ethTicker.last_price)}</span>
                    {' '}
                    <span className={isUp ? styles.livePriceUp : styles.livePriceDown}>
                      {isUp ? '↑' : '↓'} {isUp ? '+' : ''}{pct.toFixed(1)}%
                    </span>
                  </span>
                );
              })()}
            </div>
          )}
        </div>

        <div className={styles.heroRight} aria-hidden="true">
          {/* Stylized trading terminal mockup — static, deterministic */}
          <svg viewBox="0 0 480 320" className={styles.heroSvg} xmlns="http://www.w3.org/2000/svg">
            {/* Terminal frame */}
            <rect x="0" y="0" width="480" height="320" rx="8" fill="#1e2329" stroke="#2b3139" strokeWidth="1" />
            {/* Header bar */}
            <rect x="0" y="0" width="480" height="32" rx="8" fill="#252a30" />
            <rect x="0" y="24" width="480" height="8" fill="#252a30" />
            <circle cx="16" cy="16" r="4" fill="#f6465d" opacity="0.7" />
            <circle cx="30" cy="16" r="4" fill="#f0b90b" opacity="0.7" />
            <circle cx="44" cy="16" r="4" fill="#0ecb81" opacity="0.7" />
            <text x="72" y="20" fill="#eaecef" fontSize="11" fontFamily="monospace">BTCUSDT</text>
            <text x="150" y="20" fill="#0ecb81" fontSize="11" fontFamily="monospace">67,410.00</text>
            {/* Chart area */}
            <rect x="12" y="40" width="320" height="200" rx="4" fill="#0b0e11" />
            {/*
              Candlestick geometry (SVG Y-axis goes DOWN):
              - Green candle (price went UP): close Y < open Y  → close < open numerically
              - Red candle (price went DOWN): close Y > open Y  → close > open numerically
              All values are SVG coordinates within the chart area (40–240 range).
              Pattern: upward trend overall with alternating up/down candles.
            */}
            {[
              // x,  o(pen),  c(lose),  h(igh),  l(ow),  up
              { x: 30,  o: 190, c: 175, h: 170, l: 195, up: true  },  // green: c < o
              { x: 50,  o: 178, c: 185, h: 172, l: 190, up: false },  // red:   c > o
              { x: 70,  o: 182, c: 165, h: 160, l: 188, up: true  },
              { x: 90,  o: 168, c: 175, h: 162, l: 180, up: false },
              { x: 110, o: 172, c: 155, h: 150, l: 178, up: true  },
              { x: 130, o: 158, c: 165, h: 152, l: 170, up: false },
              { x: 150, o: 162, c: 145, h: 140, l: 168, up: true  },
              { x: 170, o: 148, c: 155, h: 143, l: 160, up: false },
              { x: 190, o: 152, c: 135, h: 130, l: 158, up: true  },
              { x: 210, o: 138, c: 145, h: 133, l: 150, up: false },
              { x: 230, o: 142, c: 125, h: 120, l: 148, up: true  },
              { x: 250, o: 128, c: 115, h: 110, l: 134, up: true  },
              { x: 270, o: 118, c: 128, h: 112, l: 133, up: false },
              { x: 290, o: 125, c: 112, h: 108, l: 130, up: true  },
              { x: 310, o: 115, c: 122, h: 110, l: 128, up: false },
            ].map((c, i) => {
              const color = c.up ? '#0ecb81' : '#f6465d';
              // bodyTop is the lesser of the two Y coords (visually higher on screen)
              const bodyTop = Math.min(c.o, c.c) + 40;
              const bodyH = Math.abs(c.c - c.o) || 2;
              return (
                <g key={i}>
                  {/* Wick: from high (lowest Y) to low (highest Y) */}
                  <line x1={c.x} y1={c.h + 40} x2={c.x} y2={c.l + 40} stroke={color} strokeWidth="1" opacity="0.8" />
                  <rect x={c.x - 5} y={bodyTop} width="10" height={bodyH} fill={color} rx="1" opacity="0.9" />
                </g>
              );
            })}
            {/* Volume bars at bottom of chart — static heights */}
            {[30, 50, 70, 90, 110, 130, 150, 170, 190, 210, 230, 250, 270, 290, 310].map((x, i) => {
              const h = VOLUME_HEIGHTS[i] ?? 15;
              return (
                <rect
                  key={`v${i}`}
                  x={x - 4}
                  y={240 - h}
                  width="8"
                  height={h}
                  fill={i % 2 === 0 ? '#0ecb81' : '#f6465d'}
                  opacity="0.3"
                  rx="1"
                />
              );
            })}
            {/* Order book panel */}
            <rect x="340" y="40" width="128" height="200" rx="4" fill="#0b0e11" />
            {/* Ask rows (red) — static widths and qty */}
            {[0, 1, 2, 3, 4].map((i) => (
              <g key={`a${i}`}>
                <rect x="342" y={48 + i * 18} width={ASK_WIDTHS[i]} height="14" fill="#f6465d" opacity={0.12 + i * 0.03} rx="1" />
                <text x="346" y={58 + i * 18} fill="#f6465d" fontSize="9" fontFamily="monospace" opacity="0.8">67,{510 - i * 10}</text>
                <text x="420" y={58 + i * 18} fill="#848e9c" fontSize="9" fontFamily="monospace">{ASK_QTYS[i]}</text>
              </g>
            ))}
            {/* Spread */}
            <text x="370" y={48 + 5 * 18 + 5} fill="#eaecef" fontSize="9" fontFamily="monospace" textAnchor="middle">67,460</text>
            {/* Bid rows (green) — static widths and qty */}
            {[0, 1, 2, 3, 4].map((i) => (
              <g key={`b${i}`}>
                <rect x="342" y={155 + i * 18} width={BID_WIDTHS[i]} height="14" fill="#0ecb81" opacity={0.15 - i * 0.02} rx="1" />
                <text x="346" y={165 + i * 18} fill="#0ecb81" fontSize="9" fontFamily="monospace" opacity="0.8">67,{450 - i * 10}</text>
                <text x="420" y={165 + i * 18} fill="#848e9c" fontSize="9" fontFamily="monospace">{BID_QTYS[i]}</text>
              </g>
            ))}
            {/* Order entry area */}
            <rect x="12" y="248" width="456" height="60" rx="4" fill="#252a30" />
            <rect x="20" y="256" width="100" height="28" rx="4" fill="#0ecb81" opacity="0.9" />
            <text x="48" y="274" fill="#fff" fontSize="11" fontWeight="bold" fontFamily="sans-serif">Buy</text>
            <rect x="128" y="256" width="100" height="28" rx="4" fill="#f6465d" opacity="0.9" />
            <text x="158" y="274" fill="#fff" fontSize="11" fontWeight="bold" fontFamily="sans-serif">Sell</text>
            <rect x="244" y="256" width="100" height="12" rx="3" fill="#1e2329" />
            <rect x="244" y="272" width="100" height="12" rx="3" fill="#1e2329" />
            <rect x="356" y="256" width="104" height="28" rx="4" fill="#0ecb81" />
            <text x="381" y="274" fill="#fff" fontSize="10" fontWeight="bold" fontFamily="sans-serif">Place Order</text>
            {/* Glow effect */}
            <defs>
              <radialGradient id="glow" cx="50%" cy="40%" r="50%">
                <stop offset="0%" stopColor="#2ee5a6" stopOpacity="0.08" />
                <stop offset="100%" stopColor="#2ee5a6" stopOpacity="0" />
              </radialGradient>
            </defs>
            <rect x="0" y="0" width="480" height="320" rx="8" fill="url(#glow)" />
          </svg>
        </div>
      </section>

      {/* ── Market Ticker — always visible, skeleton when loading ── */}
      <section className={styles.ticker} aria-label="Market prices">
        <div className={styles.tickerScroll}>
          <div className={styles.tickerInner}>
            {tickerLoading
              ? Array.from({ length: 8 }, (_, i) => (
                  <div key={`sk${i}`} className={styles.tickerSkeleton} aria-hidden="true" />
                ))
              : tickers.map((t) => {
                  const pct = parseFloat(t.price_change_pct_24h);
                  const isUp = pct >= 0;
                  return (
                    <button
                      key={t.symbol}
                      className={styles.tickerItem}
                      onClick={() => navigate('trade')}
                      type="button"
                    >
                      <span className={styles.tickerSymbol}>{stripQuote(t.symbol)}</span>
                      <div className={styles.tickerPriceGroup}>
                        <span className={styles.tickerPrice}>${formatPrice(t.last_price)}</span>
                        <span className={`${styles.tickerChange} ${isUp ? styles.tickerUp : styles.tickerDown}`}>
                          {isUp ? '+' : ''}{pct.toFixed(2)}%
                        </span>
                      </div>
                    </button>
                  );
                })}
          </div>
        </div>
      </section>

      {/* ── Features ── */}
      <section className={styles.section} aria-label="Platform features" data-reveal>
        <div className={styles.sectionHeader}>
          <h2 className={styles.sectionTitle}>Why Trade on Exchange</h2>
          <p className={styles.sectionSubtitle}>
            Built for professional traders who demand reliability, transparency, and speed.
          </p>
        </div>
        <div className={styles.featuresGrid}>
          {FEATURES.map((f) => (
            <div key={f.title} className={styles.featureCard}>
              <IconBox>{f.icon}</IconBox>
              <div className={styles.featureTitle}>{f.title}</div>
              <div className={styles.featureDesc}>{f.desc}</div>
            </div>
          ))}
        </div>
      </section>

      {/* ── Products ── */}
      <section className={styles.sectionAlt} aria-label="Our products" data-reveal>
        <div className={styles.sectionHeader}>
          <h2 className={styles.sectionTitle}>Our Products</h2>
          <p className={styles.sectionSubtitle}>
            Everything you need to trade, earn, and grow your portfolio in one place.
          </p>
        </div>
        <div className={styles.productsGrid}>
          {PRODUCTS.map((p) => (
            <button
              key={p.title}
              className={styles.productCard}
              onClick={() => navigate(p.path)}
              type="button"
            >
              <IconBox>{p.icon}</IconBox>
              <div className={styles.productTitle}>{p.title}</div>
              <div className={styles.productDesc}>{p.desc}</div>
              <span className={styles.productLink}>Learn More →</span>
            </button>
          ))}
        </div>
      </section>

      {/* ── Stats ── */}
      <section className={styles.section} aria-label="Platform statistics" data-reveal>
        <div className={styles.sectionHeader}>
          <h2 className={styles.sectionTitle}>Trusted by Traders</h2>
        </div>
        <div className={styles.statsRow}>
          {STATS.map((s) => (
            <div key={s.label} className={styles.statBlock}>
              <div className={styles.statValue}>{s.value}</div>
              <div className={styles.statLabel}>{s.label}</div>
            </div>
          ))}
        </div>
      </section>

      {/* ── Footer ── */}
      <footer className={styles.footer}>
        <div className={styles.footerColumns}>
          {FOOTER_COLUMNS.map((col) => (
            <div key={col.title}>
              <div className={styles.footerColTitle}>{col.title}</div>
              <div className={styles.footerColLinks}>
                {col.links.map((link) => (
                  <button
                    key={link.label}
                    className={styles.footerLink}
                    onClick={() => navigate(link.path)}
                    type="button"
                  >
                    {link.label}
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className={styles.footerBottom}>
          <span className={styles.footerCopyright}>
            © 2026 Exchange. VASP Registration #UA-2024-0317. All rights reserved.
          </span>
          <div className={styles.footerSocials}>
            <button className={styles.footerSocialBtn} type="button" aria-label="Twitter">
              <IconTwitter />
            </button>
            <button className={styles.footerSocialBtn} type="button" aria-label="Telegram">
              <IconTelegram />
            </button>
            <button className={styles.footerSocialBtn} type="button" aria-label="Discord">
              <IconDiscord />
            </button>
            <button className={styles.footerSocialBtn} type="button" aria-label="GitHub">
              <IconGitHub />
            </button>
          </div>
        </div>
      </footer>
    </div>
  );
}

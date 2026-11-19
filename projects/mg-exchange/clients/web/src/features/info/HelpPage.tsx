import { useState, useMemo } from 'react';
import { Accordion } from '@shared/ui/Accordion';
import { SearchInput } from '@shared/ui/SearchInput';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import styles from './InfoPage.module.css';

interface FaqItem {
  q: string;
  a: string;
  category: string;
}

const FAQ: FaqItem[] = [
  // Getting Started
  { category: 'getting-started', q: 'How do I create an account?', a: 'Click "Login" in the top right, then "Create Account". You need an email address and a strong password. Optional: enable 2FA immediately for security.' },
  { category: 'getting-started', q: 'What KYC level do I need?', a: 'Level 1 (email only) allows basic trading with limited withdrawals. Level 2 (ID verification) unlocks full withdrawal limits. Level 3 (enhanced due diligence) for institutional limits and higher API rate limits.' },
  { category: 'getting-started', q: 'How do I enable 2FA?', a: 'Go to Security page, click "Enable 2FA". Scan the QR code with Google Authenticator or Authy, then enter the 6-digit code to confirm. Always save your recovery codes.' },

  // Deposits & Withdrawals
  { category: 'funds', q: 'How do I deposit funds?', a: 'Go to Wallet > Deposit, select your asset and network, copy the deposit address. Deposits are credited after the required blockchain confirmations. Always verify the network matches — sending to a wrong network may result in permanent loss.' },
  { category: 'funds', q: 'How long do withdrawals take?', a: 'Crypto withdrawals are processed within 30 minutes after security review. Large withdrawals (>$50K) may require additional verification. Processing times vary by network — Bitcoin ~30 min, Ethereum ~5 min, Solana ~1 min.' },
  { category: 'funds', q: 'What are withdrawal fees?', a: 'Withdrawal fees cover blockchain network costs and vary by asset and network. See the Fees page for the current schedule. Fees are deducted from the withdrawal amount.' },

  // Trading
  { category: 'trading', q: 'What is the minimum order size?', a: 'Minimum order values vary by trading pair. Typically $1 equivalent for spot, $10 for perpetuals. Check the trading terminal for specific minimums per pair.' },
  { category: 'trading', q: 'What order types are available?', a: 'Limit, Market, Stop-Limit, Stop-Market, and Trailing Stop. Perpetual contracts also support Take Profit/Stop Loss (linked to position), OCO (one-cancels-other), and bracket orders. See the trading terminal for all options.' },
  { category: 'trading', q: 'What are the trading fees?', a: 'Fees depend on your VIP tier (based on 30-day volume or balance). Regular: 0.10% maker/taker for spot, 0.02%/0.055% for perps. Fees decrease with volume. Supreme tier makers receive rebates. See the Fees page.' },
  { category: 'trading', q: 'How does leverage work?', a: 'Perpetual contracts support up to 100x leverage. Higher leverage means less margin required but higher liquidation risk. Set leverage in the order panel before placing your order. Cross margin uses your full account balance; isolated margin is per-position.' },

  // Perpetuals
  { category: 'perps', q: 'How is funding rate calculated?', a: 'Funding is settled every 8 hours using a premium index: rate = clamp((perp_price - index_price) / index_price, -0.75%, +0.75%). Positive rate means longs pay shorts (perp trades above spot). Negative rate means shorts pay longs.' },
  { category: 'perps', q: 'What is liquidation?', a: 'When your position margin falls below the maintenance margin requirement, the exchange closes your position to prevent further losses. Liquidation price depends on your entry price, leverage, and margin mode. The liquidation engine attempts partial liquidation first.' },
  { category: 'perps', q: 'What is ADL (Auto-Deleveraging)?', a: 'When a liquidation results in a loss exceeding the insurance fund, the most profitable opposing positions are force-closed to cover the deficit. Your ADL risk is shown as a 5-bar indicator in the positions panel.' },

  // API
  { category: 'api', q: 'Can I use the API for automated trading?', a: 'Yes, go to Security > API Keys to create keys with specific permissions (read-only, trading, withdrawal). We support REST and WebSocket APIs. See the API Docs page for full reference with examples.' },
  { category: 'api', q: 'What are the API rate limits?', a: 'REST: 20 requests/second for order endpoints, 50/second for market data. WebSocket: 20 subscriptions per connection. VIP tiers have higher limits. Rate limit headers are included in every response.' },

  // Security
  { category: 'security', q: 'How are my funds protected?', a: '80% of assets are in cold storage (air-gapped, multi-signature). Hot wallet holds only 5% for immediate withdrawals. We have an insurance fund to cover liquidation shortfalls. Proof of Reserves is published regularly.' },
  { category: 'security', q: 'What should I do if my account is compromised?', a: 'Immediately: 1) Disable API keys, 2) Change password, 3) Contact support. We recommend enabling 2FA, using a unique password, and reviewing your active sessions regularly in Security settings.' },
];

const CATEGORIES: TabDef[] = [
  { key: 'all', label: 'All' },
  { key: 'getting-started', label: 'Getting Started' },
  { key: 'funds', label: 'Deposits & Withdrawals' },
  { key: 'trading', label: 'Trading' },
  { key: 'perps', label: 'Perpetuals' },
  { key: 'api', label: 'API' },
  { key: 'security', label: 'Security' },
];

export default function HelpPage() {
  const [search, setSearch] = useState('');
  const [category, setCategory] = useState('all');

  const filtered = useMemo(() => {
    let items = FAQ;
    if (category !== 'all') {
      items = items.filter((f) => f.category === category);
    }
    if (search) {
      const q = search.toLowerCase();
      items = items.filter((f) => f.q.toLowerCase().includes(q) || f.a.toLowerCase().includes(q));
    }
    return items;
  }, [search, category]);

  const accordionItems = filtered.map((item, i) => ({
    key: String(i),
    title: item.q,
    content: <p className={styles.faqAnswer}>{item.a}</p>,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Help & FAQ</h1>
      <SearchInput
        value={search}
        onChange={setSearch}
        placeholder="Search FAQ..."
        className={styles.searchBox}
      />
      <Tabs tabs={CATEGORIES} activeKey={category} onChange={setCategory} />
      <Accordion items={accordionItems} />
      {filtered.length === 0 && (
        <p className={styles.emptyFaq}>No results found. Try a different search or category.</p>
      )}
    </div>
  );
}

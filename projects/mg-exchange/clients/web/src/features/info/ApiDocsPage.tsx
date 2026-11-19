import { useState, useCallback } from 'react';
import { Badge, Button, Card, SearchInput, Toggle } from '@shared/ui';
import styles from './ApiDocsPage.module.css';

type HttpMethod = 'GET' | 'POST' | 'PUT' | 'DELETE';

interface Endpoint {
  method: HttpMethod;
  path: string;
  desc: string;
  /** Default request body for POST/PUT endpoints */
  body?: string;
}

interface Category {
  label: string;
  endpoints: Endpoint[];
}

const CATEGORIES: Category[] = [
  {
    label: 'Auth',
    endpoints: [
      { method: 'POST', path: '/api/v1/auth/register', desc: 'Create a new user account', body: '{\n  "email": "user@example.com",\n  "password": "secret",\n  "username": "trader1"\n}' },
      { method: 'POST', path: '/api/v1/auth/login', desc: 'Authenticate and receive a JWT token', body: '{\n  "email": "user@example.com",\n  "password": "secret"\n}' },
      { method: 'POST', path: '/api/v1/auth/logout', desc: 'Invalidate the current session token' },
      { method: 'POST', path: '/api/v1/auth/refresh', desc: 'Refresh an expiring JWT token' },
      { method: 'POST', path: '/api/v1/auth/2fa/enable', desc: 'Enable TOTP two-factor authentication' },
    ],
  },
  {
    label: 'Markets',
    endpoints: [
      { method: 'GET', path: '/api/v1/markets', desc: 'List all markets with prices and 24h statistics' },
      { method: 'GET', path: '/api/v1/markets/{symbol}/ticker', desc: 'Get real-time ticker for a specific symbol' },
      { method: 'GET', path: '/api/v1/markets/{symbol}/orderbook', desc: 'Get order book depth (default depth=25)' },
      { method: 'GET', path: '/api/v1/markets/{symbol}/klines', desc: 'Get OHLCV candlestick data (interval, limit params)' },
      { method: 'GET', path: '/api/v1/markets/{symbol}/trades', desc: 'Get recent public trades for a symbol' },
      { method: 'GET', path: '/api/v1/markets/index-prices', desc: 'Get index prices for all perpetual contracts' },
    ],
  },
  {
    label: 'Trading',
    endpoints: [
      { method: 'POST', path: '/api/v1/orders', desc: 'Place a new order (limit, market, stop-limit, stop-market)', body: '{\n  "symbol": "BTCUSDT",\n  "side": "Buy",\n  "order_type": "Limit",\n  "qty": "0.01",\n  "price": "60000",\n  "time_in_force": "GTC"\n}' },
      { method: 'GET', path: '/api/v1/orders', desc: 'List open orders, optionally filtered by symbol' },
      { method: 'DELETE', path: '/api/v1/orders/{order_id}', desc: 'Cancel a specific open order by ID' },
      { method: 'POST', path: '/api/v1/orders/cancel-all', desc: 'Cancel all open orders (optionally for one symbol)', body: '{\n  "symbol": "BTCUSDT"\n}' },
      { method: 'GET', path: '/api/v1/orders/history', desc: 'Paginated order history (filled, cancelled, rejected)' },
      { method: 'GET', path: '/api/v1/orders/trades', desc: 'Paginated trade fill history' },
    ],
  },
  {
    label: 'Positions',
    endpoints: [
      { method: 'GET', path: '/api/v1/positions', desc: 'Get all open futures positions with PnL' },
      { method: 'POST', path: '/api/v1/positions/{symbol}/leverage', desc: 'Set leverage for a symbol (1–125x)', body: '{\n  "leverage": 10\n}' },
      { method: 'POST', path: '/api/v1/positions/{symbol}/margin', desc: 'Add or remove isolated margin', body: '{\n  "delta": "100"\n}' },
      { method: 'POST', path: '/api/v1/positions/{symbol}/close', desc: 'Close a position at market price', body: '{\n  "symbol": "BTCUSDT"\n}' },
    ],
  },
  {
    label: 'Account',
    endpoints: [
      { method: 'GET', path: '/api/v1/account/balances', desc: 'Get per-asset balances (available, locked, total)' },
      { method: 'GET', path: '/api/v1/account/overview', desc: 'Portfolio summary — equity, margin, PnL, leverage' },
      { method: 'GET', path: '/api/v1/account/api-keys', desc: 'List API keys for the current account' },
      { method: 'POST', path: '/api/v1/account/api-keys', desc: 'Create a new API key with permission flags', body: '{\n  "label": "Trading Bot",\n  "permissions": ["read", "trade"]\n}' },
      { method: 'DELETE', path: '/api/v1/account/api-keys/{key_id}', desc: 'Revoke an API key' },
    ],
  },
  {
    label: 'Wallet',
    endpoints: [
      { method: 'GET', path: '/api/v1/wallet/balances', desc: 'Wallet balances across all assets' },
      { method: 'GET', path: '/api/v1/wallet/deposit/address', desc: 'Get deposit address for asset and network' },
      { method: 'POST', path: '/api/v1/wallet/withdraw', desc: 'Submit a withdrawal request', body: '{\n  "asset": "USDT",\n  "network": "ERC20",\n  "address": "0xabc...",\n  "amount": "100"\n}' },
      { method: 'GET', path: '/api/v1/wallet/deposits', desc: 'Paginated deposit history' },
      { method: 'GET', path: '/api/v1/wallet/withdrawals', desc: 'Paginated withdrawal history' },
    ],
  },
  {
    label: 'Earn',
    endpoints: [
      { method: 'GET', path: '/api/v1/earn/products', desc: 'List available savings and staking products' },
      { method: 'POST', path: '/api/v1/earn/subscribe', desc: 'Subscribe to an earn product', body: '{\n  "product_id": "usdt-flexible",\n  "amount": "1000"\n}' },
      { method: 'POST', path: '/api/v1/earn/redeem', desc: 'Redeem from an earn product', body: '{\n  "product_id": "usdt-flexible",\n  "amount": "500"\n}' },
      { method: 'GET', path: '/api/v1/earn/positions', desc: 'Get active earn positions with accrued yield' },
    ],
  },
  {
    label: 'Options',
    endpoints: [
      { method: 'GET', path: '/api/v1/options/instruments', desc: 'List all options instruments (calls and puts)' },
      { method: 'GET', path: '/api/v1/options/volatility', desc: 'Get implied volatility surface data' },
      { method: 'POST', path: '/api/v1/options/orders', desc: 'Place an options order', body: '{\n  "instrument": "BTC-28JUN-70000-C",\n  "side": "Buy",\n  "qty": "1",\n  "price": "2500"\n}' },
    ],
  },
  {
    label: 'Copy Trading',
    endpoints: [
      { method: 'GET', path: '/api/v1/copy-trading/masters', desc: 'List top traders available to copy' },
      { method: 'POST', path: '/api/v1/copy-trading/follow', desc: 'Start copying a master trader', body: '{\n  "master_user_id": "uuid",\n  "copy_ratio": "0.5",\n  "max_position_usdt": "1000"\n}' },
      { method: 'POST', path: '/api/v1/copy-trading/unfollow', desc: 'Stop copying a master trader', body: '{\n  "master_user_id": "uuid"\n}' },
      { method: 'GET', path: '/api/v1/copy-trading/followers', desc: 'Get followers of the current master account' },
    ],
  },
  {
    label: 'Bots',
    endpoints: [
      { method: 'GET', path: '/api/v1/bots', desc: 'List all configured trading bots' },
      { method: 'POST', path: '/api/v1/bots/grid', desc: 'Create a grid bot', body: '{\n  "symbol": "BTCUSDT",\n  "lower_price": "55000",\n  "upper_price": "70000",\n  "grid_count": 20,\n  "total_investment": "1000"\n}' },
      { method: 'POST', path: '/api/v1/bots/dca', desc: 'Create a DCA bot', body: '{\n  "symbol": "BTCUSDT",\n  "investment_per_cycle": "100",\n  "interval_hours": 24\n}' },
      { method: 'DELETE', path: '/api/v1/bots/{bot_id}', desc: 'Stop and delete a bot' },
    ],
  },
  {
    label: 'Algo',
    endpoints: [
      { method: 'POST', path: '/api/v1/algo/twap', desc: 'Start a TWAP execution strategy', body: '{\n  "symbol": "BTCUSDT",\n  "side": "Buy",\n  "total_qty": "1.0",\n  "duration_minutes": 60,\n  "slices": 12\n}' },
      { method: 'POST', path: '/api/v1/algo/vwap', desc: 'Start a VWAP execution strategy', body: '{\n  "symbol": "BTCUSDT",\n  "side": "Sell",\n  "total_qty": "0.5",\n  "duration_minutes": 30\n}' },
      { method: 'DELETE', path: '/api/v1/algo/{algo_id}', desc: 'Cancel a running algo execution' },
    ],
  },
  {
    label: 'P2P',
    endpoints: [
      { method: 'GET', path: '/api/v1/p2p/ads', desc: 'List active P2P buy/sell advertisements' },
      { method: 'POST', path: '/api/v1/p2p/ads', desc: 'Create a new P2P advertisement', body: '{\n  "side": "Sell",\n  "asset": "USDT",\n  "price": "1.01",\n  "min_qty": "100",\n  "max_qty": "10000"\n}' },
      { method: 'POST', path: '/api/v1/p2p/orders', desc: 'Create a P2P trade order against an ad', body: '{\n  "ad_id": "uuid",\n  "qty": "500"\n}' },
    ],
  },
  {
    label: 'Web3',
    endpoints: [
      { method: 'GET', path: '/api/v1/web3/wallets', desc: 'List connected Web3 wallet addresses' },
      { method: 'POST', path: '/api/v1/web3/wallets/connect', desc: 'Connect a Web3 wallet via signed message', body: '{\n  "address": "0xabc...",\n  "signature": "0x...",\n  "message": "Connect wallet to exchange"\n}' },
      { method: 'POST', path: '/api/v1/web3/wallets/disconnect', desc: 'Disconnect a Web3 wallet address' },
    ],
  },
  {
    label: 'DeFi',
    endpoints: [
      { method: 'GET', path: '/api/v1/defi/pools', desc: 'List available yield farming pools' },
      { method: 'POST', path: '/api/v1/defi/pools/{pool_id}/deposit', desc: 'Deposit liquidity into a DeFi pool', body: '{\n  "amount": "1000",\n  "asset": "USDC"\n}' },
      { method: 'POST', path: '/api/v1/defi/pools/{pool_id}/withdraw', desc: 'Withdraw liquidity from a DeFi pool', body: '{\n  "amount": "500"\n}' },
    ],
  },
  {
    label: 'Admin',
    endpoints: [
      { method: 'GET', path: '/api/admin/metrics', desc: 'System metrics — orders/sec, P99 latency, queue depths' },
      { method: 'GET', path: '/api/admin/users', desc: 'Paginated user list with KYC and balance summary' },
      { method: 'POST', path: '/api/admin/users/{user_id}/suspend', desc: 'Suspend a user account', body: '{\n  "reason": "Compliance hold"\n}' },
      { method: 'GET', path: '/api/admin/audit-log', desc: 'Paginated admin audit log' },
    ],
  },
];

const METHOD_VARIANT: Record<HttpMethod, 'success' | 'info' | 'warning' | 'danger'> = {
  GET: 'success',
  POST: 'info',
  PUT: 'warning',
  DELETE: 'danger',
};

interface TryItState {
  loading: boolean;
  statusCode: number | null;
  response: string | null;
  error: string | null;
}

interface TryItPanelProps {
  endpoint: Endpoint;
}

function TryItPanel({ endpoint }: TryItPanelProps) {
  const [useAuth, setUseAuth] = useState(false);
  const [body, setBody] = useState(endpoint.body ?? '');
  const [state, setState] = useState<TryItState>({
    loading: false,
    statusCode: null,
    response: null,
    error: null,
  });

  const canHaveBody = endpoint.method === 'POST' || endpoint.method === 'PUT';

  const handleSend = useCallback(async () => {
    setState({ loading: true, statusCode: null, response: null, error: null });

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };

    if (useAuth) {
      const token = sessionStorage.getItem('auth_token') ?? '';
      if (token) {
        headers['Authorization'] = `Bearer ${token}`;
      }
    }

    try {
      const res = await fetch(endpoint.path, {
        method: endpoint.method,
        headers,
        body: canHaveBody && body.trim() ? body : undefined,
      });

      let text = await res.text();
      try {
        text = JSON.stringify(JSON.parse(text), null, 2);
      } catch {
        // leave as raw text
      }

      setState({
        loading: false,
        statusCode: res.status,
        response: text,
        error: null,
      });
    } catch (err) {
      setState({
        loading: false,
        statusCode: null,
        response: null,
        error: err instanceof Error ? err.message : 'Network error',
      });
    }
  }, [endpoint.path, endpoint.method, canHaveBody, body, useAuth]);

  const statusVariant =
    state.statusCode == null
      ? 'neutral'
      : state.statusCode < 300
        ? 'success'
        : state.statusCode < 500
          ? 'warning'
          : 'danger';

  return (
    <div className={styles.tryItPanel}>
      <div className={styles.tryItHeader}>
        <Badge variant={METHOD_VARIANT[endpoint.method]} size="sm">
          {endpoint.method}
        </Badge>
        <code className={styles.tryItPath}>{endpoint.path}</code>
      </div>

      <div className={styles.tryItOptions}>
        <Toggle
          checked={useAuth}
          onChange={setUseAuth}
          label="Bearer auth"
          size="sm"
        />
        {useAuth && !sessionStorage.getItem('auth_token') && (
          <span className={styles.authHint}>No token in sessionStorage</span>
        )}
      </div>

      {canHaveBody && (
        <div className={styles.tryItBodySection}>
          <span className={styles.tryItLabel}>Request body</span>
          <textarea
            className={styles.bodyEditor}
            value={body}
            onChange={(e) => setBody(e.target.value)}
            spellCheck={false}
            rows={6}
            placeholder="{}"
          />
        </div>
      )}

      <Button
        variant="primary"
        size="sm"
        loading={state.loading}
        onClick={() => { void handleSend(); }}
      >
        Send request
      </Button>

      {(state.response !== null || state.error !== null) && (
        <div className={styles.responseBlock}>
          <div className={styles.responseHeader}>
            <span className={styles.tryItLabel}>Response</span>
            {state.statusCode !== null && (
              <Badge variant={statusVariant} size="sm">
                {state.statusCode}
              </Badge>
            )}
          </div>
          <pre className={styles.responseBody}>
            {state.error ?? state.response}
          </pre>
        </div>
      )}
    </div>
  );
}

export default function ApiDocsPage() {
  const [activeCategory, setActiveCategory] = useState(CATEGORIES[0].label);
  const [selectedEndpoint, setSelectedEndpoint] = useState<Endpoint | null>(null);
  const [search, setSearch] = useState('');

  const category = CATEGORIES.find((c) => c.label === activeCategory) ?? CATEGORIES[0];

  const filteredEndpoints = search.trim()
    ? category.endpoints.filter(
        (ep) =>
          ep.path.toLowerCase().includes(search.toLowerCase()) ||
          ep.desc.toLowerCase().includes(search.toLowerCase()),
      )
    : category.endpoints;

  function handleSelectEndpoint(ep: Endpoint) {
    setSelectedEndpoint((prev) =>
      prev?.path === ep.path && prev?.method === ep.method ? null : ep,
    );
  }

  function handleCategoryChange(label: string) {
    setActiveCategory(label);
    setSelectedEndpoint(null);
    setSearch('');
  }

  return (
    <div className={styles.page}>
      <div className={styles.pageHeader}>
        <h1 className={styles.title}>API Reference</h1>
        <p className={styles.subtitle}>
          Interactive REST API explorer — click any endpoint to try it live.
        </p>
      </div>

      <div className={styles.layout}>
        {/* Sidebar */}
        <nav className={styles.sidebar} aria-label="API categories">
          {CATEGORIES.map((c) => (
            <button
              key={c.label}
              type="button"
              className={`${styles.sidebarItem} ${activeCategory === c.label ? styles.sidebarItemActive : ''}`}
              onClick={() => handleCategoryChange(c.label)}
            >
              <span className={styles.sidebarLabel}>{c.label}</span>
              <span className={styles.sidebarCount}>{c.endpoints.length}</span>
            </button>
          ))}
        </nav>

        {/* Endpoint list + try-it panel */}
        <div className={styles.main}>
          <div className={styles.listHeader}>
            <h2 className={styles.categoryTitle}>{activeCategory}</h2>
            <div className={styles.searchWrap}>
              <SearchInput
                value={search}
                onChange={setSearch}
                placeholder="Filter endpoints..."
                size="sm"
              />
            </div>
          </div>

          <div className={styles.endpointList}>
            {filteredEndpoints.length === 0 && (
              <p className={styles.emptyMsg}>No endpoints match your filter.</p>
            )}
            {filteredEndpoints.map((ep) => {
              const isOpen =
                selectedEndpoint?.path === ep.path &&
                selectedEndpoint?.method === ep.method;
              return (
                <Card
                  key={`${ep.method}-${ep.path}`}
                  variant={isOpen ? 'elevated' : 'default'}
                  padding="none"
                  className={`${styles.endpointCard} ${isOpen ? styles.endpointCardOpen : ''}`}
                >
                  <button
                    type="button"
                    className={styles.endpointRow}
                    onClick={() => handleSelectEndpoint(ep)}
                    aria-expanded={isOpen}
                  >
                    <Badge variant={METHOD_VARIANT[ep.method]} size="sm">
                      {ep.method}
                    </Badge>
                    <code className={styles.endpointPath}>{ep.path}</code>
                    <span className={styles.endpointDesc}>{ep.desc}</span>
                    <span className={styles.chevron} aria-hidden="true">
                      {isOpen ? '▲' : '▼'}
                    </span>
                  </button>

                  {isOpen && <TryItPanel endpoint={ep} />}
                </Card>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}

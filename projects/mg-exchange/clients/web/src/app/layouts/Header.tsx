import { useState, useEffect } from 'react';
import { navigate } from '../Router';
import { Button } from '@shared/ui/Button';
import { Badge } from '@shared/ui/Badge';
import { useAuthStore } from '@features/account/stores/auth';
import { get } from '@shared/api/client';
import styles from './Header.module.css';

const DERIVATIVES_PATHS = ['options', 'delivery', 'margin', 'portfolio-margin', 'spread'];
const MORE_PATHS = ['algo', 'block-trade', 'launchpool', 'leveraged-tokens', 'referral', 'token'];
const ACCOUNT_PATHS = [
  'dashboard', 'wallet', 'settings', 'security', 'kyc', 'history', 'pnl',
  'sub-accounts', 'notifications', 'activity', 'address-book', 'rewards',
  'achievements', 'account-mode',
];

function DropdownItem({ path, label, current }: { path: string; label: string; current: string }) {
  return (
    <button
      className={`${styles.dropdownItem} ${current === path ? styles.dropdownItemActive : ''}`}
      onClick={() => navigate(path)}
      type="button"
      role="menuitem"
    >
      {label}
    </button>
  );
}

function getPath() { return window.location.pathname.replace(/^\//, ''); }

export function Header() {
  const { isLoggedIn, email, logout } = useAuthStore();
  const [currentPage, setCurrentPage] = useState(getPath);
  const [isTestnet, setIsTestnet] = useState(false);
  const [unreadCount, setUnreadCount] = useState(0);

  useEffect(() => {
    function sync() { setCurrentPage(getPath()); }
    window.addEventListener('popstate', sync);
    return () => window.removeEventListener('popstate', sync);
  }, []);

  useEffect(() => {
    get<{ testnet: boolean }>('/api/v1/testnet/status')
      .then((res) => setIsTestnet(res.testnet))
      .catch(() => {});
  }, []);

  // Poll unread notification count every 30 seconds
  useEffect(() => {
    if (!isLoggedIn) return;
    function fetchCount() {
      get<{ count: number }>('/account/notifications/unread-count')
        .then((res) => setUnreadCount(res.count))
        .catch(() => {});
    }
    fetchCount();
    const id = setInterval(fetchCount, 30_000);
    return () => clearInterval(id);
  }, [isLoggedIn]);

  const isDerivativesActive = DERIVATIVES_PATHS.includes(currentPage);
  const isMoreActive = MORE_PATHS.includes(currentPage);
  const isAccountActive = ACCOUNT_PATHS.includes(currentPage);

  function navClass(path: string): string {
    return `${styles.navLink} ${currentPage === path ? styles.navLinkActive : ''}`;
  }

  return (
    <header className={styles.header}>
      <span
        className={styles.logo}
        onClick={() => navigate('')}
        role="link"
        tabIndex={0}
        onKeyDown={(e) => e.key === 'Enter' && navigate('')}
      >
        Exchange
      </span>
      {isTestnet && (
        <Badge variant="warning">TESTNET</Badge>
      )}

      <nav className={styles.nav} aria-label="Main navigation">
        <button className={navClass('trade')} onClick={() => navigate('trade')} type="button">
          Trade
        </button>
        <button className={navClass('markets')} onClick={() => navigate('markets')} type="button">
          Markets
        </button>

        <div className={styles.dropdown}>
          <button
            className={`${styles.navLink} ${isDerivativesActive ? styles.navLinkActive : ''} ${styles.dropdownTrigger}`}
            type="button"
            aria-haspopup="true"
          >
            Derivatives ▾
          </button>
          <div className={styles.dropdownMenu} role="menu">
            <DropdownItem path="options" label="Options" current={currentPage} />
            <DropdownItem path="delivery" label="Delivery Futures" current={currentPage} />
            <DropdownItem path="margin" label="Spot Margin" current={currentPage} />
            <DropdownItem path="portfolio-margin" label="Portfolio Margin" current={currentPage} />
            <DropdownItem path="spread" label="Spread Trading" current={currentPage} />
          </div>
        </div>

        <button className={navClass('earn')} onClick={() => navigate('earn')} type="button">
          Earn
        </button>
        <button className={navClass('bots')} onClick={() => navigate('bots')} type="button">
          Bots
        </button>
        <button className={navClass('copy')} onClick={() => navigate('copy')} type="button">
          Copy
        </button>
        <button className={navClass('web3')} onClick={() => navigate('web3')} type="button">
          Web3
        </button>
        <button className={navClass('convert')} onClick={() => navigate('convert')} type="button">
          Convert
        </button>
        <button className={navClass('buy-crypto')} onClick={() => navigate('buy-crypto')} type="button">
          Buy Crypto
        </button>

        <button className={navClass('p2p')} onClick={() => navigate('p2p')} type="button">
          P2P
        </button>
        <button className={navClass('launchpad')} onClick={() => navigate('launchpad')} type="button">
          Launchpad
        </button>
        <button className={navClass('competitions')} onClick={() => navigate('competitions')} type="button">
          Competitions
        </button>

        <div className={styles.dropdown}>
          <button
            className={`${styles.navLink} ${isMoreActive ? styles.navLinkActive : ''} ${styles.dropdownTrigger}`}
            type="button"
            aria-haspopup="true"
          >
            More ▾
          </button>
          <div className={styles.dropdownMenu} role="menu">
            <button
              className={`${styles.dropdownItem} ${currentPage === 'algo' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('algo')}
              type="button"
              role="menuitem"
            >
              Algo Execution
            </button>
            <button
              className={`${styles.dropdownItem} ${currentPage === 'block-trade' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('block-trade')}
              type="button"
              role="menuitem"
            >
              Block Trading
            </button>
            <button
              className={`${styles.dropdownItem} ${currentPage === 'spread' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('spread')}
              type="button"
              role="menuitem"
            >
              Spread Trading
            </button>
            <button
              className={`${styles.dropdownItem} ${currentPage === 'launchpool' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('launchpool')}
              type="button"
              role="menuitem"
            >
              Launchpool
            </button>
            <button
              className={`${styles.dropdownItem} ${currentPage === 'leveraged-tokens' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('leveraged-tokens')}
              type="button"
              role="menuitem"
            >
              Leveraged Tokens
            </button>
            <button
              className={`${styles.dropdownItem} ${currentPage === 'referral' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('referral')}
              type="button"
              role="menuitem"
            >
              Referral
            </button>
            <button
              className={`${styles.dropdownItem} ${currentPage === 'token' ? styles.dropdownItemActive : ''}`}
              onClick={() => navigate('token')}
              type="button"
              role="menuitem"
            >
              EXC Token
            </button>
          </div>
        </div>

        {isLoggedIn && (
          <div className={styles.dropdown}>
            <button
              className={`${styles.navLink} ${isAccountActive ? styles.navLinkActive : ''} ${styles.dropdownTrigger}`}
              type="button"
              aria-haspopup="true"
            >
              Account ▾
            </button>
            <div className={styles.dropdownMenu} role="menu">
              <DropdownItem path="dashboard" label="Dashboard" current={currentPage} />
              <DropdownItem path="wallet" label="Wallet" current={currentPage} />
              <DropdownItem path="settings" label="Settings" current={currentPage} />
              <DropdownItem path="security" label="Security" current={currentPage} />
              <DropdownItem path="kyc" label="KYC Verification" current={currentPage} />
              <DropdownItem path="history" label="Order History" current={currentPage} />
              <DropdownItem path="pnl" label="PnL Report" current={currentPage} />
              <DropdownItem path="sub-accounts" label="Sub-Accounts" current={currentPage} />
              <DropdownItem path="notifications" label="Notifications" current={currentPage} />
              <DropdownItem path="activity" label="Activity Log" current={currentPage} />
              <DropdownItem path="address-book" label="Address Book" current={currentPage} />
              <DropdownItem path="rewards" label="Rewards" current={currentPage} />
              <DropdownItem path="achievements" label="Achievements" current={currentPage} />
              <DropdownItem path="account-mode" label="Account Mode" current={currentPage} />
            </div>
          </div>
        )}
      </nav>

      <div className={styles.headerRight}>
        {isLoggedIn ? (
          <>
            <button
              className={styles.bellBtn}
              type="button"
              onClick={() => navigate('notifications')}
              aria-label={`Notifications${unreadCount > 0 ? ` (${unreadCount} unread)` : ''}`}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
                <path d="M13.73 21a2 2 0 0 1-3.46 0" />
              </svg>
              {unreadCount > 0 && (
                <span className={styles.bellBadge}>{unreadCount > 99 ? '99+' : unreadCount}</span>
              )}
            </button>
            {email && <span className={styles.emailBadge}>{email}</span>}
            <Button variant="ghost" size="sm" onClick={logout}>
              Logout
            </Button>
          </>
        ) : (
          <Button variant="primary" size="sm" onClick={() => navigate('auth')}>
            Login
          </Button>
        )}
      </div>
    </header>
  );
}

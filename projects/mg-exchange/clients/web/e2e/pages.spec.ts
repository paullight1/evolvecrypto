import { test, expect } from '@playwright/test';

// All routes defined in src/app/routes.ts — uses History API (clean paths)
const pages = [
  { path: '/', name: 'Landing' },
  { path: '/trade', name: 'Trading Terminal' },
  { path: '/auth', name: 'Auth' },
  { path: '/options', name: 'Options' },
  { path: '/delivery', name: 'Delivery' },
  { path: '/margin', name: 'Margin' },
  { path: '/portfolio-margin', name: 'Portfolio Margin' },
  { path: '/spread', name: 'Spread' },
  { path: '/dashboard', name: 'Dashboard' },
  { path: '/markets', name: 'Markets' },
  { path: '/earn', name: 'Earn' },
  { path: '/bots', name: 'Bots' },
  { path: '/copy', name: 'Copy Trading' },
  { path: '/algo', name: 'Algo' },
  { path: '/web3', name: 'Web3' },
  { path: '/convert', name: 'Convert' },
  { path: '/buy-crypto', name: 'Buy Crypto' },
  { path: '/block-trade', name: 'Block Trade' },
  { path: '/wallet', name: 'Wallet' },
  { path: '/settings', name: 'Settings' },
  { path: '/security', name: 'Security' },
  { path: '/admin', name: 'Admin' },
  { path: '/sub-accounts', name: 'Sub Accounts' },
  { path: '/kyc', name: 'KYC' },
  { path: '/history', name: 'History' },
  { path: '/pnl', name: 'PnL' },
  { path: '/landing', name: 'Landing (alias)' },
  { path: '/fees', name: 'Fees' },
  { path: '/status', name: 'Status' },
  { path: '/help', name: 'Help' },
  { path: '/legal', name: 'Legal' },
  { path: '/proof-of-reserves', name: 'Proof of Reserves' },
  { path: '/insurance-fund', name: 'Insurance Fund' },
  { path: '/api-docs', name: 'API Docs' },
  { path: '/p2p', name: 'P2P' },
  { path: '/launchpad', name: 'Launchpad' },
  { path: '/launchpool', name: 'Launchpool' },
  { path: '/leveraged-tokens', name: 'Leveraged Tokens' },
  { path: '/competitions', name: 'Competitions' },
  { path: '/referral', name: 'Referral' },
  { path: '/token', name: 'Token' },
  { path: '/social', name: 'Social' },
  { path: '/activity', name: 'Activity Log' },
  { path: '/address-book', name: 'Address Book' },
  { path: '/notifications', name: 'Notifications' },
  { path: '/achievements', name: 'Achievements' },
  { path: '/rewards', name: 'Rewards' },
];

// Paths that Vite also exposes as proxy prefixes (to the backend).
// Direct browser navigation to these paths returns a proxy error instead of index.html.
// These must be reached via in-app pushState navigation after loading the SPA root.
const PROXIED_PREFIXES = ['/admin', '/api'];

function isProxied(path: string): boolean {
  return PROXIED_PREFIXES.some((prefix) => path === prefix || path.startsWith(prefix + '/') || path.startsWith(prefix + '-'));
}

test.describe('All Pages Load Without Crash', () => {
  for (const { path, name } of pages) {
    test(`${name} page loads without error`, async ({ page }) => {
      const jsErrors: string[] = [];
      page.on('pageerror', (err) => {
        // Ignore network/fetch errors — they happen when backend is not running
        // and are handled gracefully by .catch() in API calls.
        const msg = err.message || '';
        if (msg.includes('fetch') || msg.includes('NetworkError') || msg.includes('Failed to fetch') || msg.includes('ECONNREFUSED')) return;
        jsErrors.push(msg);
      });

      if (isProxied(path)) {
        // Load the SPA root first, then use the app's History API router to navigate.
        // This avoids Vite intercepting the direct URL request as a backend proxy call.
        await page.goto('/');
        await page.waitForLoadState('networkidle');
        await page.evaluate((p) => {
          window.history.pushState(null, '', p);
          window.dispatchEvent(new PopStateEvent('popstate'));
        }, path);
      } else {
        await page.goto(path);
        await page.waitForLoadState('networkidle');
      }

      // Wait for the React app root to have rendered at least one child element.
      // This resolves any pending Suspense lazy-load before checking content.
      await page.locator('#root > *').first().waitFor({ state: 'attached', timeout: 10000 });

      // Page must have non-empty body content — use innerHTML so spinner markup
      // (which has no text) also counts as rendered content.
      const bodyHtml = await page.locator('body').innerHTML();
      expect(bodyHtml.length).toBeGreaterThan(0);

      // No uncaught JS errors should have occurred
      expect(jsErrors).toHaveLength(0);
    });
  }
});

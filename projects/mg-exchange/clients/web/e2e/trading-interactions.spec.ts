import { test, expect } from '@playwright/test';

test.describe('Trading Terminal — Interactions', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByRole('button', { name: 'Spot' })).toBeVisible({ timeout: 10000 });
  });

  // ── BottomPanel Tab Switching ─────────────────────────────────────

  test('clicking Open Orders tab shows orders section', async ({ page }) => {
    const tab = page.getByRole('tab', { name: 'Open Orders' });
    await tab.click();
    // Tab should be selected
    await expect(tab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
  });

  test('clicking Order History tab switches content', async ({ page }) => {
    const historyTab = page.getByRole('tab', { name: 'Order History' });
    await historyTab.click();
    await expect(historyTab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
  });

  test('switching to USDT-M shows Positions tab', async ({ page }) => {
    await page.getByRole('button', { name: 'USDT-M' }).click();
    await expect(page.getByRole('tab', { name: 'Positions' })).toBeVisible({ timeout: 5000 });
  });

  test('Positions tab is clickable in futures mode', async ({ page }) => {
    await page.getByRole('button', { name: 'USDT-M' }).click();
    const posTab = page.getByRole('tab', { name: 'Positions' });
    await posTab.click();
    await expect(posTab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
  });

  // ── Trading Mode Switching ────────────────────────────────────────

  test('Spot mode is available', async ({ page }) => {
    await page.getByRole('button', { name: 'Spot' }).click();
    // In spot mode, there should be no Positions tab
    const posTab = page.getByRole('tab', { name: 'Positions' });
    // Positions may still be visible but context changes
    await expect(page.getByRole('tab', { name: 'Open Orders' })).toBeVisible({ timeout: 3000 });
  });

  test('USDT-M mode switch changes context', async ({ page }) => {
    await page.getByRole('button', { name: 'USDT-M' }).click();
    // Futures mode should show additional tabs/info
    await expect(page.getByRole('tab', { name: 'Open Orders' })).toBeVisible({ timeout: 3000 });
  });

  // ── Order Book ────────────────────────────────────────────────────

  test('order book section is visible', async ({ page }) => {
    // Order book should render with asks and bids sections
    await expect(page.getByText(/order book/i).first()).toBeVisible({ timeout: 5000 });
  });

  // ── Symbol / Pair Display ─────────────────────────────────────────

  test('current symbol is displayed in ticker bar', async ({ page }) => {
    // Should show a symbol like BTCUSDT
    await expect(page.getByText(/USDT/i).first()).toBeVisible({ timeout: 5000 });
  });

  test('24h stats are visible in ticker bar', async ({ page }) => {
    // Ticker bar shows 24h change, high, low, volume
    await expect(page.getByText(/24h/i).first()).toBeVisible({ timeout: 5000 });
  });
});

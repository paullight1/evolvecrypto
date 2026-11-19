import { test, expect } from '@playwright/test';

test.describe('Trading Terminal', () => {
  test('trading page renders the terminal shell', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.locator('body')).not.toBeEmpty();
  });

  test('TickerBar shows trading mode tabs', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByRole('button', { name: 'Spot' })).toBeVisible({ timeout: 10000 });
    await expect(page.getByRole('button', { name: 'USDT-M' })).toBeVisible({ timeout: 10000 });
  });

  test('TickerBar shows a symbol name with BTC', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByText(/BTC/i).first()).toBeVisible({ timeout: 10000 });
  });

  test('OrderEntry has Buy and Sell side buttons', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByRole('button', { name: /buy/i }).first()).toBeVisible({ timeout: 10000 });
    await expect(page.getByRole('button', { name: /sell/i }).first()).toBeVisible({ timeout: 10000 });
  });

  test('OrderEntry order type tabs are rendered', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByRole('button', { name: 'Limit', exact: true })).toBeVisible({ timeout: 10000 });
    await expect(page.getByRole('button', { name: 'Market', exact: true })).toBeVisible({ timeout: 10000 });
  });

  test('BottomPanel tabs are rendered', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByRole('tab', { name: 'Open Orders' })).toBeVisible({ timeout: 10000 });
    await expect(page.getByRole('tab', { name: 'Order History' })).toBeVisible({ timeout: 10000 });
  });

  test('switching trading mode tabs works', async ({ page }) => {
    await page.goto('/trade');
    const futuresTab = page.getByRole('button', { name: 'USDT-M' });
    await futuresTab.waitFor({ state: 'visible', timeout: 10000 });
    await futuresTab.click();
    await expect(page.getByRole('tab', { name: 'Positions' })).toBeVisible({ timeout: 10000 });
  });
});

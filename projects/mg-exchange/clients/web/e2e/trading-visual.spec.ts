import { test, expect } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';

test('trading terminal has all panels visible', async ({ page }) => {
  await page.goto('/trade');
  await page.waitForTimeout(4000);

  // Chart area exists
  const chartPanel = page.locator('[class*="panel"]').first();
  await chartPanel.waitFor({ state: 'visible', timeout: 10000 });

  // OrderBook exists
  const orderBookTitle = page.getByText('Order Book').first();
  expect(await orderBookTitle.isVisible()).toBe(true);

  // OrderEntry has buy/sell buttons visible
  const buyBtn = page.getByRole('button', { name: /Buy/i }).first();
  expect(await buyBtn.isVisible()).toBe(true);

  // BottomPanel tabs are visible
  const ordersTab = page.getByRole('tab', { name: /Open Orders/i }).first();
  expect(await ordersTab.isVisible()).toBe(true);

  // BottomPanel is full width
  const ordersTabBox = await ordersTab.boundingBox();
  expect(ordersTabBox?.x).toBeLessThan(400);

  // Ensure test-results directory exists for screenshot
  const dir = path.join(process.cwd(), 'test-results');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });

  await page.screenshot({ path: 'test-results/trading-terminal.png' });
});

test('ticker bar shows all required stats', async ({ page }) => {
  await page.goto('/trade');
  await page.waitForTimeout(4000);

  await expect(page.getByRole('button', { name: 'Spot' })).toBeVisible({ timeout: 10000 });
  await expect(page.getByRole('button', { name: 'USDT-M' })).toBeVisible({ timeout: 10000 });
  await expect(page.getByRole('button', { name: 'COIN-M' })).toBeVisible({ timeout: 10000 });

  await expect(page.getByText('24h High')).toBeVisible();
  await expect(page.getByText('24h Low')).toBeVisible();
  await expect(page.getByText('24h Vol')).toBeVisible();
});

test('order book has data rows', async ({ page }) => {
  await page.goto('/trade');
  await page.waitForTimeout(3000);

  await expect(page.getByText('Order Book').first()).toBeVisible({ timeout: 10000 });

  const askPrices = page.locator('[class*="priceAsk"]');
  await expect(askPrices.first()).toBeVisible({ timeout: 10000 });

  const bidPrices = page.locator('[class*="priceBid"]');
  await expect(bidPrices.first()).toBeVisible({ timeout: 10000 });
});

test('bottom panel spans full width', async ({ page }) => {
  await page.goto('/trade');
  await page.waitForTimeout(3000);

  const ordersTab = page.getByRole('tab', { name: /Open Orders/i }).first();
  await ordersTab.waitFor({ state: 'visible', timeout: 10000 });
  const tabBox = await ordersTab.boundingBox();

  expect(tabBox?.x).toBeLessThan(300);
});

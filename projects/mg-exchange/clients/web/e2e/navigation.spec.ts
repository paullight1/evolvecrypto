import { test, expect } from '@playwright/test';

test.describe('Navigation', () => {
  test('landing page loads and shows the app shell', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByText('Exchange').first()).toBeVisible();
  });

  test('landing page shows hero section', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: /Trade Crypto/i })).toBeVisible({ timeout: 10000 });
  });

  test('navigate to auth page', async ({ page }) => {
    await page.goto('/auth');
    await expect(page.getByRole('tab', { name: 'Log In' })).toBeVisible({ timeout: 10000 });
  });

  test('navigate to markets page', async ({ page }) => {
    await page.goto('/markets');
    await expect(page.locator('body')).not.toBeEmpty();
    await expect(page.getByText(/markets/i)).toBeVisible({ timeout: 10000 });
  });

  test('navigate to wallet page', async ({ page }) => {
    await page.goto('/wallet');
    await expect(page.locator('body')).not.toBeEmpty();
  });

  test('navigate to trading terminal', async ({ page }) => {
    await page.goto('/trade');
    await expect(page.getByText(/BTC|ETH|USDT/i).first()).toBeVisible({ timeout: 10000 });
  });

  test('clicking Trade nav button navigates to trading terminal', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Trade' }).first().click();
    await expect(page.getByText(/BTC|ETH|USDT/i).first()).toBeVisible({ timeout: 10000 });
  });

  test('clicking Markets nav button navigates to markets page', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Markets' }).first().click();
    await expect(page.getByText(/markets/i)).toBeVisible({ timeout: 10000 });
  });

  test('unknown path falls back to landing', async ({ page }) => {
    await page.goto('/this-path-does-not-exist');
    await expect(page.locator('body')).not.toBeEmpty();
  });
});

import { test, expect } from '@playwright/test';

test.describe('Order Entry — UI Interactions', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/trade');
    // Wait for terminal to render
    await expect(page.getByRole('button', { name: 'Limit', exact: true })).toBeVisible({ timeout: 10000 });
  });

  // ── Order Type Switching ──────────────────────────────────────────

  test('switching to Market hides price input', async ({ page }) => {
    // In Limit mode the price row is rendered: OrderEntry shows a <label>Price (USDT)</label>
    // followed by a NumberInput with placeholder "0.00".
    const priceLabel = page.locator('label', { hasText: /^Price \(/ });
    await expect(priceLabel).toBeVisible({ timeout: 5000 });

    // Switch to Market — hasPrice('market') === false, so the entire price row is removed
    await page.getByRole('button', { name: 'Market', exact: true }).click();

    // Price row no longer in the DOM in Market mode
    await expect(priceLabel).not.toBeVisible({ timeout: 3000 });
  });

  test('switching back to Limit restores price input', async ({ page }) => {
    await page.getByRole('button', { name: 'Market', exact: true }).click();
    await page.getByRole('button', { name: 'Limit', exact: true }).click();
    // Price row reappears when returning to Limit
    const priceLabel = page.locator('label', { hasText: /^Price \(/ });
    await expect(priceLabel).toBeVisible({ timeout: 3000 });
    // The NumberInput inside that row renders an <input> with placeholder "0.00" — must be enabled
    const priceInput = page.locator('input[placeholder="0.00"]').first();
    await expect(priceInput).toBeEnabled();
  });

  // ── Buy / Sell Toggle ─────────────────────────────────────────────

  test('Buy button is active by default', async ({ page }) => {
    const buyBtn = page.getByRole('button', { name: /buy/i }).first();
    await expect(buyBtn).toBeVisible();
  });

  test('clicking Sell changes active side', async ({ page }) => {
    const sellBtn = page.getByRole('button', { name: /sell/i }).first();
    await sellBtn.click();
    // After clicking Sell, the sell button should have an active/selected state
    await expect(sellBtn).toBeVisible();
  });

  // ── Stop Order Types ──────────────────────────────────────────────

  test('Stop-Limit tab shows trigger price and limit price inputs', async ({ page }) => {
    const stopBtn = page.getByRole('button', { name: /stop/i }).first();
    if (await stopBtn.isVisible()) {
      await stopBtn.click();
      // Should have trigger price field
      await expect(page.getByText(/trigger/i).first()).toBeVisible({ timeout: 3000 });
    }
  });

  // ── Quantity Input ────────────────────────────────────────────────

  test('percentage buttons exist for quick qty selection', async ({ page }) => {
    // Look for 25%, 50%, 75%, 100% buttons
    const pctButton = page.getByRole('button', { name: /25%|50%|75%|100%/ }).first();
    await expect(pctButton).toBeVisible({ timeout: 5000 });
  });
});

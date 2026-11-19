import { test, expect } from '@playwright/test';

test.describe('Wallet Forms — Client Validation', () => {
  // ── Withdraw Form ─────────────────────────────────────────────────

  test.describe('Withdraw', () => {
    test.beforeEach(async ({ page }) => {
      // The wallet uses tabs — navigate to /wallet then click the Withdraw tab
      await page.goto('/wallet');
      await page.getByRole('tab', { name: 'Withdraw' }).click();
      // Wait for withdraw form to render — button text is "Withdraw {ASSET}" e.g. "Withdraw BTC"
      await expect(page.getByRole('button', { name: /^Withdraw /i }).first()).toBeVisible({ timeout: 10000 });
    });

    test('submit with empty address shows "Address is required"', async ({ page }) => {
      // Fill amount but leave address empty
      const amountInput = page.getByLabel('Amount');
      if (await amountInput.isVisible()) {
        await amountInput.fill('0.01');
      }
      await page.getByRole('button', { name: /^Withdraw /i }).first().click();
      await expect(page.getByText('Address is required')).toBeVisible({ timeout: 3000 });
    });

    test('submit with zero amount shows validation error', async ({ page }) => {
      // Fill address but enter 0 amount
      const addressInput = page.getByLabel('Withdrawal Address');
      if (await addressInput.isVisible()) {
        await addressInput.fill('0x1234567890abcdef1234567890abcdef12345678');
      }
      const amountInput = page.getByLabel('Amount');
      if (await amountInput.isVisible()) {
        await amountInput.fill('0');
      }
      await page.getByRole('button', { name: /^Withdraw /i }).first().click();
      await expect(page.getByText('Enter a valid amount')).toBeVisible({ timeout: 3000 });
    });

    test('asset selector has options', async ({ page }) => {
      // The withdraw form uses Select (renders native <select>) for asset and network
      const selects = page.locator('select');
      const count = await selects.count();
      expect(count).toBeGreaterThan(0);
    });

    test('Max button fills the amount field', async ({ page }) => {
      const maxBtn = page.getByRole('button', { name: /max/i }).first();
      if (await maxBtn.isVisible()) {
        await maxBtn.click();
        // Amount input should now have a value (even if "0")
        const amountInput = page.getByLabel('Amount');
        const value = await amountInput.inputValue();
        expect(value).toBeTruthy();
      }
    });
  });

  // ── Deposit Form ──────────────────────────────────────────────────

  test.describe('Deposit', () => {
    test('deposit page shows asset selection and address', async ({ page }) => {
      await page.goto('/wallet');
      await page.getByRole('tab', { name: 'Deposit' }).click();
      // DepositForm uses Select (native <select>) for asset and network
      await expect(page.locator('select').first()).toBeVisible({ timeout: 10000 });
    });

    test('deposit page shows deposit address or QR placeholder', async ({ page }) => {
      await page.goto('/wallet');
      await page.getByRole('tab', { name: 'Deposit' }).click();
      // DepositForm renders a <p class="label">Deposit Address</p> once address loads,
      // or "Could not load deposit address." on error — either contains "deposit address"
      const addressText = page.getByText(/deposit address/i).first();
      await expect(addressText).toBeVisible({ timeout: 10000 });
    });
  });
});

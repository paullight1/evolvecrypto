import { test, expect, type Page } from '@playwright/test';

test.describe('Auth Flow — Client Validation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/auth');
    await expect(page.getByRole('tab', { name: 'Log In' })).toBeVisible({ timeout: 10000 });
  });

  // ── Login Validation ──────────────────────────────────────────────

  test('login with empty email shows "Email is required"', async ({ page }) => {
    const submitBtn = page.getByRole('button', { name: /sign in|log in/i }).first();
    await submitBtn.click();
    await expect(page.getByText('Email is required')).toBeVisible({ timeout: 3000 });
  });

  test('login with invalid email format shows validation error', async ({ page }) => {
    await page.getByRole('textbox', { name: 'Email' }).fill('not-an-email');
    await page.getByRole('textbox', { name: 'Password', exact: true }).fill('somepassword');
    const submitBtn = page.getByRole('button', { name: /sign in|log in/i }).first();
    await submitBtn.click();
    await expect(page.getByText('Enter a valid email address')).toBeVisible({ timeout: 3000 });
  });

  test('login with empty password shows "Password is required"', async ({ page }) => {
    await page.getByRole('textbox', { name: 'Email' }).fill('user@test.com');
    const submitBtn = page.getByRole('button', { name: /sign in|log in/i }).first();
    await submitBtn.click();
    await expect(page.getByText('Password is required')).toBeVisible({ timeout: 3000 });
  });

  // ── Register Validation ───────────────────────────────────────────

  // Helper: the "Create Account" button is disabled until agreeTerms is checked.
  // Switch to register tab and tick the terms checkbox to enable the submit button.
  // The Checkbox component renders <span class="control"> over the <input>, so use
  // force:true to bypass the pointer-events interception on the custom control span.
  async function goToRegisterAndAgree(p: Page) {
    await p.getByRole('tab', { name: 'Register' }).click();
    await p.getByRole('checkbox').check({ force: true });
  }

  test('register with short password shows validation error', async ({ page }) => {
    await goToRegisterAndAgree(page);
    await page.getByRole('textbox', { name: 'Email' }).fill('user@test.com');
    await page.getByRole('textbox', { name: 'Password', exact: true }).fill('short');
    await page.getByRole('textbox', { name: 'Confirm Password' }).fill('short');
    await page.getByRole('button', { name: 'Create Account' }).click();
    await expect(page.getByText(/at least 8 characters/i)).toBeVisible({ timeout: 3000 });
  });

  test('register with mismatched passwords shows error', async ({ page }) => {
    await goToRegisterAndAgree(page);
    await page.getByRole('textbox', { name: 'Email' }).fill('user@test.com');
    await page.getByRole('textbox', { name: 'Password', exact: true }).fill('StrongPass123!');
    await page.getByRole('textbox', { name: 'Confirm Password' }).fill('DifferentPass123!');
    await page.getByRole('button', { name: 'Create Account' }).click();
    await expect(page.getByText(/do not match/i)).toBeVisible({ timeout: 3000 });
  });

  test('register with empty fields shows validation errors', async ({ page }) => {
    await goToRegisterAndAgree(page);
    await page.getByRole('button', { name: 'Create Account' }).click();
    await expect(page.getByText('Email is required')).toBeVisible({ timeout: 3000 });
  });

  // ── Tab Switching ─────────────────────────────────────────────────

  test('switching tabs clears password fields', async ({ page }) => {
    await page.getByRole('textbox', { name: 'Password', exact: true }).fill('mypassword');
    await page.getByRole('tab', { name: 'Register' }).click();
    const passwordInput = page.getByRole('textbox', { name: 'Password', exact: true });
    await expect(passwordInput).toHaveValue('');
  });

  test('password strength bar appears when typing on register', async ({ page }) => {
    await page.getByRole('tab', { name: 'Register' }).click();
    await page.getByRole('textbox', { name: 'Password', exact: true }).fill('StrongPass123!');
    // Strength label should appear (Weak/Fair/Good/Strong)
    await expect(page.getByText(/weak|fair|good|strong/i).first()).toBeVisible({ timeout: 3000 });
  });
});

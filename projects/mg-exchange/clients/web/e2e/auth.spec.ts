import { test, expect } from '@playwright/test';

test.describe('Authentication', () => {
  test('auth page shows Log In and Register tabs', async ({ page }) => {
    await page.goto('/auth');
    await expect(page.getByRole('tab', { name: 'Log In' })).toBeVisible({ timeout: 10000 });
    await expect(page.getByRole('tab', { name: 'Register' })).toBeVisible();
  });

  test('login form has email and password inputs', async ({ page }) => {
    await page.goto('/auth');
    await expect(page.getByRole('tab', { name: 'Log In' })).toBeVisible({ timeout: 10000 });
    // Auth page has email + password inputs
    await expect(page.locator('input[type="email"], input[placeholder*="email" i]').first()).toBeVisible({ timeout: 5000 });
    await expect(page.locator('input[type="password"]').first()).toBeVisible();
  });

  test('switching to Register tab shows Create Account button', async ({ page }) => {
    await page.goto('/auth');
    await page.getByRole('tab', { name: 'Register' }).click();
    await expect(page.getByRole('button', { name: 'Create Account' })).toBeVisible({ timeout: 5000 });
  });

  test('login with empty fields shows validation error', async ({ page }) => {
    await page.goto('/auth');
    await expect(page.getByRole('tab', { name: 'Log In' })).toBeVisible({ timeout: 10000 });
    // Find and click the sign in / login submit button
    const signInBtn = page.getByRole('button', { name: /sign in|log in/i }).first();
    await signInBtn.click();
    await expect(page.getByText(/email.*required|please enter/i)).toBeVisible({ timeout: 3000 });
  });

  test('register form shows all required fields', async ({ page }) => {
    await page.goto('/auth');
    await page.getByRole('tab', { name: 'Register' }).click();
    // Register form has email, password, confirm password, and terms checkbox
    await expect(page.getByRole('textbox', { name: 'Email' })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole('textbox', { name: 'Password', exact: true })).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'Confirm Password' })).toBeVisible();
    await expect(page.getByRole('checkbox')).toBeVisible();
    // Create Account button exists (may be disabled until form is valid)
    await expect(page.getByRole('button', { name: 'Create Account' })).toBeVisible();
  });
});

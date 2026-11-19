import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  setup2FA: vi.fn(),
  enable2FA: vi.fn(),
  changePassword: vi.fn(),
  getApiKeys: vi.fn(),
  createApiKey: vi.fn(),
  revokeApiKey: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  refreshToken: vi.fn(),
  disable2FA: vi.fn(),
  getSessions: vi.fn(),
  revokeSession: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}));

import { SecuritySettings } from './SecuritySettings';
import { setup2FA, enable2FA, changePassword } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';

const mockSetup2FA = vi.mocked(setup2FA);
const mockEnable2FA = vi.mocked(enable2FA);
const mockChangePassword = vi.mocked(changePassword);

beforeEach(() => {
  vi.clearAllMocks();
  mockSetup2FA.mockResolvedValue({ secret: 'JBSWY3DPEHPK3PXP', qr_url: 'otpauth://totp/Exchange:user@test.com' });
  mockEnable2FA.mockResolvedValue(undefined);
  mockChangePassword.mockResolvedValue(undefined);
  useToastStore.setState({ toasts: [] });
});

describe('SecuritySettings', () => {
  it('renders 2FA section and Setup 2FA button', async () => {
    await act(async () => { render(<SecuritySettings />); });

    expect(screen.getByText('Two-Factor Authentication')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Setup 2FA' })).toBeInTheDocument();
  });

  it('opens 2FA setup modal when Setup 2FA is clicked', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SecuritySettings />); });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Setup 2FA' }));
    });

    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Setup 2FA' })).toBeInTheDocument();
    });
  });

  it('renders the Change Password form', async () => {
    await act(async () => { render(<SecuritySettings />); });

    expect(screen.getByLabelText('Current Password')).toBeInTheDocument();
    expect(screen.getByLabelText('New Password')).toBeInTheDocument();
    expect(screen.getByLabelText('Confirm New Password')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Change Password' })).toBeInTheDocument();
  });

  it('shows validation errors when password fields are empty', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SecuritySettings />); });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Change Password' }));
    });

    await waitFor(() => {
      expect(screen.getByText('Current password required')).toBeInTheDocument();
    });

    expect(mockChangePassword).not.toHaveBeenCalled();
  });

  it('shows mismatch error when passwords do not match', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SecuritySettings />); });

    await user.type(screen.getByLabelText('Current Password'), 'oldpassword');
    await user.type(screen.getByLabelText('New Password'), 'newpassword1');
    await user.type(screen.getByLabelText('Confirm New Password'), 'differentpassword');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Change Password' }));
    });

    await waitFor(() => {
      expect(screen.getByText('Passwords do not match')).toBeInTheDocument();
    });

    expect(mockChangePassword).not.toHaveBeenCalled();
  });

  it('calls changePassword on valid submit', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SecuritySettings />); });

    await user.type(screen.getByLabelText('Current Password'), 'oldpassword');
    await user.type(screen.getByLabelText('New Password'), 'newpassword1');
    await user.type(screen.getByLabelText('Confirm New Password'), 'newpassword1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Change Password' }));
    });

    await waitFor(() => {
      expect(mockChangePassword).toHaveBeenCalledWith({
        current_password: 'oldpassword',
        new_password: 'newpassword1',
      });
    });
  });
});

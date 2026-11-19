import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getApiKeys: vi.fn(),
  createApiKey: vi.fn(),
  revokeApiKey: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  refreshToken: vi.fn(),
  setup2FA: vi.fn(),
  enable2FA: vi.fn(),
  disable2FA: vi.fn(),
  getSessions: vi.fn(),
  revokeSession: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  changePassword: vi.fn(),
}));

import { ApiKeyManager } from './ApiKeyManager';
import { getApiKeys, createApiKey, revokeApiKey } from '../../api';
import type { ApiKey } from '../../types';

const mockGetApiKeys = vi.mocked(getApiKeys);
const mockCreateApiKey = vi.mocked(createApiKey);
const mockRevokeApiKey = vi.mocked(revokeApiKey);

const MOCK_KEYS: ApiKey[] = [
  { key: 'key-abc123', label: 'Trading Bot', permissions: ['read', 'trade'], created_at: 1700000000 },
  { key: 'key-def456', label: 'Read Only', permissions: ['read'], created_at: 1700100000 },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetApiKeys.mockResolvedValue(MOCK_KEYS);
  mockCreateApiKey.mockResolvedValue({ key: 'new-key', secret: 'secret-xyz', label: 'New Bot', permissions: ['read'], created_at: 1700200000 });
  mockRevokeApiKey.mockResolvedValue(undefined);
});

describe('ApiKeyManager', () => {
  it('renders API keys loaded from the API', async () => {
    await act(async () => { render(<ApiKeyManager />); });

    await waitFor(() => {
      expect(screen.getByText('Trading Bot')).toBeInTheDocument();
      expect(screen.getByText('Read Only')).toBeInTheDocument();
    });
  });

  it('renders the New Key button', async () => {
    await act(async () => { render(<ApiKeyManager />); });

    expect(screen.getByRole('button', { name: '+ New Key' })).toBeInTheDocument();
  });

  it('opens create modal when New Key button is clicked', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ApiKeyManager />); });

    await user.click(screen.getByRole('button', { name: '+ New Key' }));

    expect(screen.getByRole('dialog', { name: 'Create API Key' })).toBeInTheDocument();
    expect(screen.getByLabelText('Label')).toBeInTheDocument();
  });

  it('calls createApiKey when Create Key is submitted with a label', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ApiKeyManager />); });

    await user.click(screen.getByRole('button', { name: '+ New Key' }));
    await user.type(screen.getByLabelText('Label'), 'New Bot');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create Key' }));
    });

    await waitFor(() => {
      expect(mockCreateApiKey).toHaveBeenCalledWith(
        expect.objectContaining({ label: 'New Bot' }),
      );
    });
  });

  it('shows revoke confirm dialog when Revoke is clicked', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ApiKeyManager />); });

    await waitFor(() => screen.getAllByRole('button', { name: 'Revoke' }));

    await user.click(screen.getAllByRole('button', { name: 'Revoke' })[0]);

    expect(screen.getByText('This key will be permanently revoked and can no longer be used.')).toBeInTheDocument();
  });

  it('calls revokeApiKey when revoke is confirmed', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ApiKeyManager />); });

    await waitFor(() => screen.getAllByRole('button', { name: 'Revoke' }));
    // Click the first Revoke button from the table
    await user.click(screen.getAllByRole('button', { name: 'Revoke' })[0]);

    // Now the confirm dialog is open — click the Revoke button inside the dialog
    const dialog = screen.getByRole('dialog', { name: 'Revoke API Key' });
    const confirmBtn = Array.from(dialog.querySelectorAll('button')).find((b) => b.textContent?.trim() === 'Revoke');
    await act(async () => {
      await user.click(confirmBtn!);
    });

    await waitFor(() => {
      expect(mockRevokeApiKey).toHaveBeenCalled();
    });
  });
});

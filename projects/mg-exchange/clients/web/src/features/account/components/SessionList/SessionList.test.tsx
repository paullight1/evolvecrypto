import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getSessions: vi.fn(),
  revokeSession: vi.fn(),
  getApiKeys: vi.fn(),
  createApiKey: vi.fn(),
  revokeApiKey: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  refreshToken: vi.fn(),
  setup2FA: vi.fn(),
  enable2FA: vi.fn(),
  disable2FA: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  changePassword: vi.fn(),
}));

import { SessionList } from './SessionList';
import { getSessions, revokeSession } from '../../api';
import type { Session } from '../../types';

const mockGetSessions = vi.mocked(getSessions);
const mockRevokeSession = vi.mocked(revokeSession);

const MOCK_SESSIONS: Session[] = [
  { id: 'sess-1', ip: '192.168.1.1', device: 'Chrome on macOS', last_active: 1700000000 },
  { id: 'sess-2', ip: '10.0.0.2', device: 'Firefox on Windows', last_active: 1700100000 },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetSessions.mockResolvedValue(MOCK_SESSIONS);
  mockRevokeSession.mockResolvedValue(undefined);
});

describe('SessionList', () => {
  it('renders sessions loaded from the API', async () => {
    await act(async () => { render(<SessionList />); });

    await waitFor(() => {
      expect(screen.getByText('Chrome on macOS')).toBeInTheDocument();
      expect(screen.getByText('Firefox on Windows')).toBeInTheDocument();
    });
  });

  it('marks first session as current', async () => {
    await act(async () => { render(<SessionList />); });

    await waitFor(() => screen.getByText('Current'));

    expect(screen.getByText('Current')).toBeInTheDocument();
  });

  it('renders Revoke button only for non-current sessions', async () => {
    await act(async () => { render(<SessionList />); });

    await waitFor(() => screen.getAllByText('Active'));

    const revokeBtns = screen.getAllByRole('button', { name: 'Revoke' });
    expect(revokeBtns).toHaveLength(1); // only non-current session
  });

  it('shows confirm dialog when Revoke is clicked', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SessionList />); });

    await waitFor(() => screen.getByRole('button', { name: 'Revoke' }));
    await user.click(screen.getByRole('button', { name: 'Revoke' }));

    expect(screen.getByText('This will sign out the device associated with this session.')).toBeInTheDocument();
  });

  it('calls revokeSession when confirmed', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SessionList />); });

    await waitFor(() => screen.getByRole('button', { name: 'Revoke' }));
    await user.click(screen.getByRole('button', { name: 'Revoke' }));

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Revoke Session' }));
    });

    await waitFor(() => {
      expect(mockRevokeSession).toHaveBeenCalledWith('sess-2');
    });
  });
});

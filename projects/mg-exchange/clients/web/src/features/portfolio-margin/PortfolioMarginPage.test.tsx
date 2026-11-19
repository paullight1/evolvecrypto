import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getPortfolioMarginStatus: vi.fn(),
  enablePortfolioMargin: vi.fn(),
  disablePortfolioMargin: vi.fn(),
  getPortfolioAnalysis: vi.fn(),
}));

import PortfolioMarginPage from './PortfolioMarginPage';
import {
  getPortfolioMarginStatus,
  enablePortfolioMargin,
  disablePortfolioMargin,
  getPortfolioAnalysis,
} from './api';
import type { PortfolioMarginStatus, PortfolioAnalysis } from './types';

const mockGetStatus = vi.mocked(getPortfolioMarginStatus);
const mockEnable = vi.mocked(enablePortfolioMargin);
const mockDisable = vi.mocked(disablePortfolioMargin);
const mockGetAnalysis = vi.mocked(getPortfolioAnalysis);

const STATUS_DISABLED: PortfolioMarginStatus = {
  enabled: false,
  margin_level: '5.2',
  initial_margin: '10000.00',
  maintenance_margin: '7500.00',
  available_balance: '25000.00',
  total_equity: '35000.00',
};

const STATUS_ENABLED: PortfolioMarginStatus = {
  enabled: true,
  margin_level: '8.1',
  initial_margin: '8000.00',
  maintenance_margin: '6000.00',
  available_balance: '28000.00',
  total_equity: '36000.00',
};

const MOCK_ANALYSIS: PortfolioAnalysis = {
  scenarios: [
    { scenario: 'BTC -10%', pnl: '-1200.00', margin_required: '11000.00', margin_ratio: '90%' },
    { scenario: 'BTC +10%', pnl: '1200.00', margin_required: '9000.00', margin_ratio: '110%' },
  ],
  net_delta: '0.5',
  net_gamma: '0.02',
  stress_loss: '-4500.00',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetStatus.mockResolvedValue(STATUS_DISABLED);
  mockGetAnalysis.mockResolvedValue(MOCK_ANALYSIS);
  mockEnable.mockResolvedValue(STATUS_ENABLED);
  mockDisable.mockResolvedValue(STATUS_DISABLED);
});

describe('PortfolioMarginPage', () => {
  it('shows Disabled badge when portfolio margin is off', async () => {
    render(<PortfolioMarginPage />);

    expect(await screen.findByText('Disabled')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Enable Portfolio Margin' })).toBeInTheDocument();
  });

  it('enable button calls enablePortfolioMargin API after confirmation', async () => {
    render(<PortfolioMarginPage />);

    await screen.findByText('Disabled');
    await userEvent.click(screen.getByRole('button', { name: 'Enable Portfolio Margin' }));

    // ConfirmDialog appears
    const confirmBtn = await screen.findByRole('button', { name: 'Enable' });
    await act(async () => {
      await userEvent.click(confirmBtn);
    });

    expect(mockEnable).toHaveBeenCalledTimes(1);
    // UI should now show Enabled
    expect(await screen.findByText('Enabled')).toBeInTheDocument();
  });

  it('disable button calls disablePortfolioMargin after confirmation', async () => {
    mockGetStatus.mockResolvedValue(STATUS_ENABLED);
    mockDisable.mockResolvedValue(STATUS_DISABLED);
    render(<PortfolioMarginPage />);

    await screen.findByText('Enabled');
    await userEvent.click(screen.getByRole('button', { name: 'Disable Portfolio Margin' }));

    const confirmBtn = await screen.findByRole('button', { name: 'Disable' });
    await act(async () => {
      await userEvent.click(confirmBtn);
    });

    expect(mockDisable).toHaveBeenCalledTimes(1);
  });

  it('scenario analysis table renders rows', async () => {
    render(<PortfolioMarginPage />);

    expect(await screen.findByText('BTC -10%')).toBeInTheDocument();
    expect(screen.getByText('BTC +10%')).toBeInTheDocument();
  });

  it('risk summary shows net delta and stress loss', async () => {
    render(<PortfolioMarginPage />);

    await waitFor(() => {
      expect(screen.getByText('Net Delta')).toBeInTheDocument();
      expect(screen.getByText('0.5')).toBeInTheDocument();
    });
    expect(screen.getByText('-4500.00')).toBeInTheDocument();
  });
});

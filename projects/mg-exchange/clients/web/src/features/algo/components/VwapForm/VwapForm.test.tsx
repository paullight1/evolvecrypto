import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  createVwap: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { VwapForm } from './VwapForm';
import { createVwap } from '../../api';

const mockCreateVwap = vi.mocked(createVwap);

beforeEach(() => {
  vi.clearAllMocks();
  mockCreateVwap.mockResolvedValue({
    id: 'algo-2',
    type: 'vwap',
    symbol: 'BTCUSDT',
    side: 'buy',
    total_qty: '1',
    executed_qty: '0',
    duration_seconds: 3600,
    status: 'running',
    created_at: new Date().toISOString(),
  });
});

describe('VwapForm', () => {
  it('renders all form fields', async () => {
    await act(async () => { render(<VwapForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Symbol')).toBeInTheDocument();
    expect(screen.getByLabelText('Side')).toBeInTheDocument();
    expect(screen.getByLabelText('Quantity')).toBeInTheDocument();
    expect(screen.getByLabelText('Duration (minutes)')).toBeInTheDocument();
  });

  it('shows toast error when qty is zero', async () => {
    const { toast } = await import('@shared/ui/Toast');
    const user = userEvent.setup();
    await act(async () => { render(<VwapForm onCreated={vi.fn()} />); });

    await user.click(screen.getByRole('button', { name: 'Create VWAP Order' }));

    expect(toast.error).toHaveBeenCalledWith('Enter a valid quantity');
    expect(mockCreateVwap).not.toHaveBeenCalled();
  });

  it('shows toast error when duration is zero', async () => {
    const { toast } = await import('@shared/ui/Toast');
    const user = userEvent.setup();
    await act(async () => { render(<VwapForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Quantity'), '1');

    const durInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durInput);
    await user.type(durInput, '0');

    await user.click(screen.getByRole('button', { name: 'Create VWAP Order' }));

    expect(toast.error).toHaveBeenCalledWith('Enter a valid duration');
  });

  it('calls createVwap with correct params on valid submit', async () => {
    const onCreated = vi.fn();
    const user = userEvent.setup();
    await act(async () => { render(<VwapForm onCreated={onCreated} />); });

    await user.type(screen.getByLabelText('Quantity'), '2');

    const durInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durInput);
    await user.type(durInput, '120');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create VWAP Order' }));
    });

    await waitFor(() => {
      expect(mockCreateVwap).toHaveBeenCalledWith(
        expect.objectContaining({
          symbol: 'BTCUSDT',
          side: 'buy',
          qty: '2',
          duration_seconds: 7200, // 120 * 60
        }),
      );
      expect(onCreated).toHaveBeenCalled();
    });
  });

  it('side can be changed to sell', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<VwapForm onCreated={vi.fn()} />); });

    const sideSelect = screen.getByLabelText('Side');
    await user.selectOptions(sideSelect, 'sell');

    expect((sideSelect as HTMLSelectElement).value).toBe('sell');
  });
});

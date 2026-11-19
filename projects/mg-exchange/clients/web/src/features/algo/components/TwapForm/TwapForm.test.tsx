import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  createTwap: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { TwapForm } from './TwapForm';
import { createTwap } from '../../api';

const mockCreateTwap = vi.mocked(createTwap);

beforeEach(() => {
  vi.clearAllMocks();
  mockCreateTwap.mockResolvedValue({
    id: 'algo-1',
    type: 'twap',
    symbol: 'BTCUSDT',
    side: 'buy',
    total_qty: '1',
    executed_qty: '0',
    duration_seconds: 3600,
    slices: 10,
    status: 'running',
    created_at: new Date().toISOString(),
  });
});

describe('TwapForm', () => {
  it('renders all form fields', async () => {
    await act(async () => { render(<TwapForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Symbol')).toBeInTheDocument();
    expect(screen.getByLabelText('Side')).toBeInTheDocument();
    expect(screen.getByLabelText('Quantity')).toBeInTheDocument();
    expect(screen.getByLabelText('Duration (minutes)')).toBeInTheDocument();
    expect(screen.getByLabelText('Slices')).toBeInTheDocument();
  });

  it('shows toast error when qty is empty', async () => {
    const { toast } = await import('@shared/ui/Toast');
    const user = userEvent.setup();
    await act(async () => { render(<TwapForm onCreated={vi.fn()} />); });

    await user.click(screen.getByRole('button', { name: 'Create TWAP Order' }));

    expect(toast.error).toHaveBeenCalledWith('Enter a valid quantity');
    expect(mockCreateTwap).not.toHaveBeenCalled();
  });

  it('shows toast error when duration is 0', async () => {
    const { toast } = await import('@shared/ui/Toast');
    const user = userEvent.setup();
    await act(async () => { render(<TwapForm onCreated={vi.fn()} />); });

    const qtyInput = screen.getByLabelText('Quantity');
    await user.type(qtyInput, '1');

    const durInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durInput);
    await user.type(durInput, '0');

    await user.click(screen.getByRole('button', { name: 'Create TWAP Order' }));

    expect(toast.error).toHaveBeenCalledWith('Enter a valid duration');
  });

  it('calls createTwap with correct params on valid submit', async () => {
    const onCreated = vi.fn();
    const user = userEvent.setup();
    await act(async () => { render(<TwapForm onCreated={onCreated} />); });

    await user.type(screen.getByLabelText('Quantity'), '0.5');

    const durInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durInput);
    await user.type(durInput, '30');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create TWAP Order' }));
    });

    await waitFor(() => {
      expect(mockCreateTwap).toHaveBeenCalledWith(
        expect.objectContaining({
          symbol: 'BTCUSDT',
          side: 'buy',
          qty: '0.5',
          duration_seconds: 1800, // 30 * 60
        }),
      );
      expect(onCreated).toHaveBeenCalled();
    });
  });

  it('renders Create TWAP Order submit button', async () => {
    await act(async () => { render(<TwapForm onCreated={vi.fn()} />); });

    expect(screen.getByRole('button', { name: 'Create TWAP Order' })).toBeInTheDocument();
  });
});

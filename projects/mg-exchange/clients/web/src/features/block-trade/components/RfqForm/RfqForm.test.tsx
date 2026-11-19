import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  createRfq: vi.fn(),
}));

import { RfqForm } from './RfqForm';
import { createRfq } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';

const mockCreateRfq = vi.mocked(createRfq);

beforeEach(() => {
  vi.clearAllMocks();
  mockCreateRfq.mockResolvedValue({ id: 'rfq-1', symbol: 'BTCUSDT', side: 'buy', qty: '1', notional_usd: '100000', status: 'open', quotes: [], created_at: '' });
  useToastStore.setState({ toasts: [] });
});

describe('RfqForm', () => {
  it('renders the minimum notional notice', async () => {
    await act(async () => { render(<RfqForm onCreated={vi.fn()} />); });

    expect(screen.getByText(/Block trading minimum.*\$100,000/)).toBeInTheDocument();
  });

  it('renders symbol, side, quantity and notional inputs', async () => {
    await act(async () => { render(<RfqForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Symbol')).toBeInTheDocument();
    expect(screen.getByLabelText('Side')).toBeInTheDocument();
    expect(screen.getByLabelText('Quantity')).toBeInTheDocument();
    expect(screen.getByLabelText('Notional (USD)')).toBeInTheDocument();
  });

  it('shows error when notional is below $100,000', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<RfqForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Quantity'), '1');
    await user.type(screen.getByLabelText('Notional (USD)'), '50000');

    expect(screen.getByText(/Minimum \$100,000/)).toBeInTheDocument();
  });

  it('shows toast error on submit with notional below minimum', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<RfqForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Quantity'), '1');
    await user.type(screen.getByLabelText('Notional (USD)'), '50000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Submit RFQ' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });

    expect(mockCreateRfq).not.toHaveBeenCalled();
  });

  it('calls createRfq and onCreated on valid submit', async () => {
    const user = userEvent.setup();
    const onCreated = vi.fn();
    await act(async () => { render(<RfqForm onCreated={onCreated} />); });

    await user.type(screen.getByLabelText('Quantity'), '2');
    await user.type(screen.getByLabelText('Notional (USD)'), '150000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Submit RFQ' }));
    });

    await waitFor(() => {
      expect(mockCreateRfq).toHaveBeenCalledWith(
        expect.objectContaining({ qty: '2', notional_usd: '150000' }),
      );
      expect(onCreated).toHaveBeenCalled();
    });
  });
});

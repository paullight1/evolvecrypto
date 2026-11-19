import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  placeSpreadOrder: vi.fn(),
}));

import { SpreadOrderForm } from './SpreadOrderForm';
import { placeSpreadOrder } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { SpreadInstrument } from '../../types';

const mockPlaceSpreadOrder = vi.mocked(placeSpreadOrder);

const MOCK_INSTRUMENT: SpreadInstrument = {
  id: 'sp-1',
  name: 'BTC-JAN25-FEB25',
  near_leg: 'BTCUSDT-JAN25',
  far_leg: 'BTCUSDT-FEB25',
  spread_value: '-150.00',
  bid: '-155.00',
  ask: '-145.00',
  last: '-148.00',
  volume_24h: '500000',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockPlaceSpreadOrder.mockResolvedValue({ order_id: 'ord-1', status: 'open' });
  useToastStore.setState({ toasts: [] });
});

describe('SpreadOrderForm', () => {
  it('shows placeholder when no instrument is selected', async () => {
    await act(async () => {
      render(<SpreadOrderForm instrument={null} />);
    });

    expect(screen.getByText(/Select a spread instrument/)).toBeInTheDocument();
  });

  it('renders instrument name when selected', async () => {
    await act(async () => {
      render(<SpreadOrderForm instrument={MOCK_INSTRUMENT} />);
    });

    expect(screen.getByText('BTC-JAN25-FEB25')).toBeInTheDocument();
  });

  it('renders quantity input', async () => {
    await act(async () => {
      render(<SpreadOrderForm instrument={MOCK_INSTRUMENT} />);
    });

    expect(screen.getByLabelText('Quantity')).toBeInTheDocument();
  });

  it('shows price input for limit order type', async () => {
    await act(async () => {
      render(<SpreadOrderForm instrument={MOCK_INSTRUMENT} />);
    });

    // Default order type is limit
    expect(screen.getByLabelText('Price')).toBeInTheDocument();
  });

  it('hides price input when market order type is selected', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<SpreadOrderForm instrument={MOCK_INSTRUMENT} />);
    });

    await user.selectOptions(screen.getByLabelText('Order Type'), 'market');

    expect(screen.queryByLabelText('Price')).not.toBeInTheDocument();
  });

  it('calls placeSpreadOrder on valid submit', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<SpreadOrderForm instrument={MOCK_INSTRUMENT} />);
    });

    await user.type(screen.getByLabelText('Quantity'), '5');
    await user.type(screen.getByLabelText('Price'), '-150');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Place Spread Order' }));
    });

    await waitFor(() => {
      expect(mockPlaceSpreadOrder).toHaveBeenCalledWith(
        expect.objectContaining({ instrument_id: 'sp-1', qty: '5', order_type: 'limit' }),
      );
    });
  });

  it('shows error toast when quantity is invalid', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<SpreadOrderForm instrument={MOCK_INSTRUMENT} />);
    });

    // Leave quantity empty, type only price
    await user.type(screen.getByLabelText('Price'), '-150');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Place Spread Order' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });

    expect(mockPlaceSpreadOrder).not.toHaveBeenCalled();
  });
});

import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  placeDeliveryOrder: vi.fn(),
}));

import { DeliveryOrderEntry } from './DeliveryOrderEntry';
import { placeDeliveryOrder } from '../../api';

const mockPlaceDeliveryOrder = vi.mocked(placeDeliveryOrder);

const INSTRUMENTS = [
  { symbol: 'BTCUSDT-0328', delivery_date: '2025-03-28T00:00:00Z' },
  { symbol: 'ETHUSDT-0328', delivery_date: '2025-03-28T00:00:00Z' },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockPlaceDeliveryOrder.mockResolvedValue(undefined);
});

describe('DeliveryOrderEntry', () => {
  it('renders instrument options', async () => {
    await act(async () => { render(<DeliveryOrderEntry instruments={INSTRUMENTS} />); });

    expect(screen.getByText('Place Delivery Order')).toBeInTheDocument();
    // The first instrument should be selected by default
    const select = screen.getByLabelText('Instrument');
    expect(select).toBeInTheDocument();
  });

  it('shows validation error when qty is empty', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryOrderEntry instruments={INSTRUMENTS} />); });

    await user.click(screen.getByRole('button', { name: 'Buy Long' }));

    expect(screen.getByText('Enter a valid quantity')).toBeInTheDocument();
    expect(mockPlaceDeliveryOrder).not.toHaveBeenCalled();
  });

  it('submit calls placeDeliveryOrder with correct data', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryOrderEntry instruments={INSTRUMENTS} />); });

    const qtyInput = screen.getByLabelText('Quantity');
    await user.clear(qtyInput);
    await user.type(qtyInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Buy Long' }));
    });

    await waitFor(() => {
      expect(mockPlaceDeliveryOrder).toHaveBeenCalledWith(
        expect.objectContaining({
          symbol: 'BTCUSDT-0328',
          side: 'buy',
          order_type: 'limit',
          qty: '0.1',
        }),
      );
    });
  });

  it('switching side changes button label to Sell Short', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryOrderEntry instruments={INSTRUMENTS} />); });

    const sideSelect = screen.getByLabelText('Side');
    await user.selectOptions(sideSelect, 'sell');

    expect(screen.getByRole('button', { name: 'Sell Short' })).toBeInTheDocument();
  });

  it('shows empty state when no instruments provided', async () => {
    await act(async () => { render(<DeliveryOrderEntry instruments={[]} />); });

    expect(screen.getByText('No instruments available')).toBeInTheDocument();
  });
});

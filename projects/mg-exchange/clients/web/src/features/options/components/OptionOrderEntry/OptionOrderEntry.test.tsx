import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  placeOptionOrder: vi.fn(),
}));

import { OptionOrderEntry } from './OptionOrderEntry';
import { placeOptionOrder } from '../../api';

const mockPlaceOptionOrder = vi.mocked(placeOptionOrder);

beforeEach(() => {
  vi.clearAllMocks();
  mockPlaceOptionOrder.mockResolvedValue({ order_id: 'opt-1', status: 'open' });
});

describe('OptionOrderEntry', () => {
  it('renders with initial buy side', async () => {
    await act(async () => { render(<OptionOrderEntry symbol="BTC-28MAR25-60000-C" />); });

    expect(screen.getByText('Buy Option')).toBeInTheDocument();
  });

  it('sell side toggle changes submit button label', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionOrderEntry symbol="BTC-28MAR25-60000-C" initialSide="buy" />); });

    // Change Side select to Sell
    const sideSelect = screen.getByLabelText('Side');
    await user.selectOptions(sideSelect, 'sell');

    expect(screen.getByText('Sell Option')).toBeInTheDocument();
  });

  it('shows validation error when qty is zero', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionOrderEntry symbol="BTC-28MAR25-60000-C" />); });

    const submitBtn = screen.getByRole('button', { name: 'Buy Option' });
    await user.click(submitBtn);

    expect(screen.getByText('Enter a valid quantity')).toBeInTheDocument();
    expect(mockPlaceOptionOrder).not.toHaveBeenCalled();
  });

  it('submit calls placeOptionOrder with correct data', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionOrderEntry symbol="BTC-28MAR25-60000-C" />); });

    const qtyInput = screen.getByLabelText('Quantity');
    await user.clear(qtyInput);
    await user.type(qtyInput, '2');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Buy Option' }));
    });

    await waitFor(() => {
      expect(mockPlaceOptionOrder).toHaveBeenCalledWith(
        expect.objectContaining({
          symbol: 'BTC-28MAR25-60000-C',
          side: 'buy',
          qty: '2',
        }),
      );
    });
  });

  it('price field hidden for market order type', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionOrderEntry symbol="BTC-28MAR25-60000-C" />); });

    const orderTypeSelect = screen.getByLabelText('Order Type');
    await user.selectOptions(orderTypeSelect, 'market');

    expect(screen.queryByLabelText('Price')).not.toBeInTheDocument();
  });

  it('shows error message on API failure', async () => {
    mockPlaceOptionOrder.mockRejectedValue(new Error('Insufficient balance'));
    const user = userEvent.setup();
    await act(async () => { render(<OptionOrderEntry symbol="BTC-28MAR25-60000-C" />); });

    const qtyInput = screen.getByLabelText('Quantity');
    await user.clear(qtyInput);
    await user.type(qtyInput, '1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Buy Option' }));
    });

    await waitFor(() => {
      expect(screen.getByText('Insufficient balance')).toBeInTheDocument();
    });
  });
});

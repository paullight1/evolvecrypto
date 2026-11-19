import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getP2PAds: vi.fn(),
  createP2POrder: vi.fn(),
}));

import { AdList } from './AdList';
import { getP2PAds, createP2POrder } from '../../api';
import type { P2PAd } from '../../types';

const mockGetP2PAds = vi.mocked(getP2PAds);
const mockCreateP2POrder = vi.mocked(createP2POrder);

const MOCK_ADS: P2PAd[] = [
  {
    id: 'ad-1',
    user_id: 'TraderAlice',
    side: 'sell',
    asset: 'USDT',
    fiat_currency: 'USD',
    price: '1.01',
    min_amount: '50',
    max_amount: '5000',
    payment_methods: ['bank_transfer', 'paypal'],
    created_at: '2023-11-14T22:13:20.000Z',
  },
  {
    id: 'ad-2',
    user_id: 'TraderBob',
    side: 'sell',
    asset: 'USDT',
    fiat_currency: 'USD',
    price: '1.02',
    min_amount: '100',
    max_amount: '10000',
    payment_methods: ['sepa'],
    created_at: '2023-11-14T22:13:20.000Z',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetP2PAds.mockResolvedValue(MOCK_ADS);
  mockCreateP2POrder.mockResolvedValue(undefined as never);
});

describe('AdList', () => {
  it('renders ads returned by the API', async () => {
    await act(async () => { render(<AdList side="buy" />); });

    await waitFor(() => {
      expect(screen.getByText('TraderAlice')).toBeInTheDocument();
      expect(screen.getByText('TraderBob')).toBeInTheDocument();
    });
  });

  it('renders asset and fiat filter selects', async () => {
    await act(async () => { render(<AdList side="buy" />); });

    expect(screen.getByLabelText('Asset')).toBeInTheDocument();
    expect(screen.getByLabelText('Fiat')).toBeInTheDocument();
  });

  it('reloads ads when asset filter changes', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdList side="buy" />); });

    await waitFor(() => expect(mockGetP2PAds).toHaveBeenCalledTimes(1));

    await user.selectOptions(screen.getByLabelText('Asset'), 'BTC');

    await waitFor(() => {
      expect(mockGetP2PAds).toHaveBeenCalledWith('buy', 'BTC', 'USD');
    });
  });

  it('opens order modal when Buy button is clicked', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdList side="buy" />); });

    await waitFor(() => screen.getAllByRole('button', { name: 'Buy' }));

    const buyBtns = screen.getAllByRole('button', { name: 'Buy' });
    await user.click(buyBtns[0]);

    expect(screen.getByRole('dialog', { name: 'Place Order' })).toBeInTheDocument();
    expect(screen.getByText(/Price:/)).toBeInTheDocument();
  });

  it('calls createP2POrder when order is confirmed', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdList side="buy" />); });

    await waitFor(() => screen.getAllByRole('button', { name: 'Buy' }));

    await user.click(screen.getAllByRole('button', { name: 'Buy' })[0]);

    const amountInput = screen.getByLabelText(/Amount/);
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: /Confirm Buy/ }));
    });

    await waitFor(() => {
      expect(mockCreateP2POrder).toHaveBeenCalledWith(
        expect.objectContaining({ ad_id: 'ad-1', amount: '100' }),
      );
    });
  });
});

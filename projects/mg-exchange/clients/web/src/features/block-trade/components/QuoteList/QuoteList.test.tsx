import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  executeRfq: vi.fn(),
}));

import { QuoteList } from './QuoteList';
import { executeRfq } from '../../api';
import type { RfqQuote } from '../../types';

const mockExecuteRfq = vi.mocked(executeRfq);

const MOCK_QUOTES: RfqQuote[] = [
  {
    id: 'q-1',
    rfq_id: 'rfq-1',
    maker: 'MarketMaker1',
    price: '65000.00',
    qty: '2',
    notional_usd: '130000',
    side: 'buy',
    expires_at: new Date(Date.now() + 60_000).toISOString(),
    status: 'pending',
  },
  {
    id: 'q-2',
    rfq_id: 'rfq-1',
    maker: 'MarketMaker2',
    price: '65100.00',
    qty: '2',
    notional_usd: '130200',
    side: 'buy',
    expires_at: new Date(Date.now() + 60_000).toISOString(),
    status: 'accepted',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockExecuteRfq.mockResolvedValue({ order_id: 'ord-1', status: 'filled' } as never);
});

describe('QuoteList', () => {
  it('renders quote rows with maker names', async () => {
    await act(async () => {
      render(<QuoteList rfqId="rfq-1" quotes={MOCK_QUOTES} loading={false} onExecuted={vi.fn()} />);
    });

    expect(screen.getByText('MarketMaker1')).toBeInTheDocument();
    expect(screen.getByText('MarketMaker2')).toBeInTheDocument();
  });

  it('shows empty state when no quotes', async () => {
    await act(async () => {
      render(<QuoteList rfqId="rfq-1" quotes={[]} loading={false} onExecuted={vi.fn()} />);
    });

    expect(screen.getByText('No quotes received yet')).toBeInTheDocument();
  });

  it('renders Execute button only for pending quotes', async () => {
    await act(async () => {
      render(<QuoteList rfqId="rfq-1" quotes={MOCK_QUOTES} loading={false} onExecuted={vi.fn()} />);
    });

    const executeBtns = screen.getAllByRole('button', { name: 'Execute' });
    expect(executeBtns).toHaveLength(1); // only q-1 is pending
  });

  it('calls executeRfq and onExecuted when Execute is clicked', async () => {
    const user = userEvent.setup();
    const onExecuted = vi.fn();
    await act(async () => {
      render(<QuoteList rfqId="rfq-1" quotes={MOCK_QUOTES} loading={false} onExecuted={onExecuted} />);
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Execute' }));
    });

    await waitFor(() => {
      expect(mockExecuteRfq).toHaveBeenCalledWith('rfq-1', 'q-1');
      expect(onExecuted).toHaveBeenCalled();
    });
  });

  it('shows status badge for accepted quote', async () => {
    await act(async () => {
      render(<QuoteList rfqId="rfq-1" quotes={MOCK_QUOTES} loading={false} onExecuted={vi.fn()} />);
    });

    expect(screen.getByText('accepted')).toBeInTheDocument();
    expect(screen.getByText('pending')).toBeInTheDocument();
  });
});

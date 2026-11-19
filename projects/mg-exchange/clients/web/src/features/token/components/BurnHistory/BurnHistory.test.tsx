import { render, screen, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { BurnHistory } from './BurnHistory';
import type { BurnRecord } from '../../types';

const MOCK_RECORDS: BurnRecord[] = [
  {
    id: 'br-1',
    amount: '500000',
    tx_hash: '0xabc123def456789012345678901234567890',
    burned_at: 1700000000,
    source: 'buyback',
  },
  {
    id: 'br-2',
    amount: '125000',
    tx_hash: '0xdef456abc123789012345678901234567890',
    burned_at: 1700100000,
    source: 'fees',
  },
];

describe('BurnHistory', () => {
  it('renders burn amounts', async () => {
    await act(async () => {
      render(<BurnHistory records={MOCK_RECORDS} loading={false} />);
    });

    expect(screen.getByText('500000')).toBeInTheDocument();
    expect(screen.getByText('125000')).toBeInTheDocument();
  });

  it('renders source badges', async () => {
    await act(async () => {
      render(<BurnHistory records={MOCK_RECORDS} loading={false} />);
    });

    expect(screen.getByText('Buyback')).toBeInTheDocument();
    expect(screen.getByText('Fee Burn')).toBeInTheDocument();
  });

  it('renders truncated tx hash', async () => {
    await act(async () => {
      render(<BurnHistory records={MOCK_RECORDS} loading={false} />);
    });

    // Hash is sliced to first 12 chars then appended with …
    expect(screen.getByText('0xabc123def4…')).toBeInTheDocument();
  });

  it('shows empty state when no records', async () => {
    await act(async () => {
      render(<BurnHistory records={[]} loading={false} />);
    });

    expect(screen.getByText('No burn records')).toBeInTheDocument();
  });

  it('renders table headers', async () => {
    await act(async () => {
      render(<BurnHistory records={MOCK_RECORDS} loading={false} />);
    });

    expect(screen.getByText('Amount (EXC)')).toBeInTheDocument();
    expect(screen.getByText('Source')).toBeInTheDocument();
    expect(screen.getByText('Tx Hash')).toBeInTheDocument();
  });
});

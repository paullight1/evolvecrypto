import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ChainSelector } from './ChainSelector';
import type { Chain } from '../../types';

const MOCK_CHAINS: Chain[] = [
  { id: 'ethereum', name: 'Ethereum', native_token: 'ETH', rpc_url: 'https://eth.rpc', explorer_url: 'https://etherscan.io' },
  { id: 'polygon', name: 'Polygon', native_token: 'MATIC', rpc_url: 'https://poly.rpc', explorer_url: 'https://polygonscan.com' },
  { id: 'arbitrum', name: 'Arbitrum', native_token: 'ETH', rpc_url: 'https://arb.rpc', explorer_url: 'https://arbiscan.io' },
];

describe('ChainSelector', () => {
  it('renders with default label "Chain"', () => {
    render(<ChainSelector chains={MOCK_CHAINS} value="ethereum" onChange={vi.fn()} />);

    expect(screen.getByLabelText('Chain')).toBeInTheDocument();
  });

  it('renders with custom label', () => {
    render(<ChainSelector chains={MOCK_CHAINS} value="ethereum" onChange={vi.fn()} label="From Chain" />);

    expect(screen.getByLabelText('From Chain')).toBeInTheDocument();
  });

  it('renders all chain options with name and native token', () => {
    render(<ChainSelector chains={MOCK_CHAINS} value="ethereum" onChange={vi.fn()} />);

    expect(screen.getByRole('option', { name: 'Ethereum (ETH)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Polygon (MATIC)' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Arbitrum (ETH)' })).toBeInTheDocument();
  });

  it('shows the currently selected chain', () => {
    render(<ChainSelector chains={MOCK_CHAINS} value="polygon" onChange={vi.fn()} />);

    const select = screen.getByLabelText('Chain') as HTMLSelectElement;
    expect(select.value).toBe('polygon');
  });

  it('selecting a different chain fires onChange with the chain id', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ChainSelector chains={MOCK_CHAINS} value="ethereum" onChange={onChange} />);

    await user.selectOptions(screen.getByLabelText('Chain'), 'arbitrum');

    expect(onChange).toHaveBeenCalledWith('arbitrum');
  });
});

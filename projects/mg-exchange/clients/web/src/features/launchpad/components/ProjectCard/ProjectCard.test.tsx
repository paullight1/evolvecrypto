import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { ProjectCard } from './ProjectCard';
import type { LaunchpadProject } from '../../types';

function makeProject(overrides: Partial<LaunchpadProject> = {}): LaunchpadProject {
  return {
    id: 'proj-1',
    token_name: 'ExampleToken',
    token_symbol: 'EXT',
    description: 'A new DeFi protocol.',
    total_raise_usdt: '1000000',
    committed_usdt: '500000',
    token_price_usdt: '0.10',
    min_commitment_usdt: '100',
    max_commitment_usdt: '10000',
    status: 'active',
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 7 * 86_400_000).toISOString(),
    claim_date: null,
    token_distribution: '50% public',
    eligibility_note: 'KYC required',
    website_url: 'https://example.com',
    ...overrides,
  };
}

describe('ProjectCard', () => {
  it('renders token name and symbol', async () => {
    await act(async () => {
      render(<ProjectCard project={makeProject()} onSelect={vi.fn()} />);
    });

    expect(screen.getByText('ExampleToken')).toBeInTheDocument();
    expect(screen.getByText('EXT')).toBeInTheDocument();
  });

  it('shows status badge', async () => {
    await act(async () => {
      render(<ProjectCard project={makeProject({ status: 'active' })} onSelect={vi.fn()} />);
    });

    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('renders progress bar for active project', async () => {
    const project = makeProject({ status: 'active', committed_usdt: '500000', total_raise_usdt: '1000000' });
    await act(async () => {
      render(<ProjectCard project={project} onSelect={vi.fn()} />);
    });

    expect(screen.getByText(/50\.0% filled/)).toBeInTheDocument();
  });

  it('shows Commit Now button for active project', async () => {
    await act(async () => {
      render(<ProjectCard project={makeProject({ status: 'active' })} onSelect={vi.fn()} />);
    });

    expect(screen.getByRole('button', { name: 'Commit Now' })).toBeInTheDocument();
  });

  it('calls onSelect when button is clicked', async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    await act(async () => {
      render(<ProjectCard project={makeProject()} onSelect={onSelect} />);
    });

    await user.click(screen.getByRole('button', { name: 'Commit Now' }));

    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'proj-1' }));
  });
});

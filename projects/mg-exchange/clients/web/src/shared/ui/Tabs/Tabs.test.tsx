import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Tabs } from './Tabs';

const tabs = [
  { key: 'a', label: 'Alpha' },
  { key: 'b', label: 'Beta', badge: 3 },
  { key: 'c', label: 'Gamma' },
];

describe('Tabs', () => {
  it('renders all tab labels', () => {
    render(<Tabs tabs={tabs} activeKey="a" onChange={() => undefined} />);
    expect(screen.getByText('Alpha')).toBeInTheDocument();
    expect(screen.getByText('Beta')).toBeInTheDocument();
    expect(screen.getByText('Gamma')).toBeInTheDocument();
  });

  it('marks the active tab with aria-selected', () => {
    render(<Tabs tabs={tabs} activeKey="b" onChange={() => undefined} />);
    const betaTab = screen.getByRole('tab', { name: /Beta/ });
    expect(betaTab).toHaveAttribute('aria-selected', 'true');
    const alphaTab = screen.getByRole('tab', { name: /Alpha/ });
    expect(alphaTab).toHaveAttribute('aria-selected', 'false');
  });

  it('calls onChange when clicking a tab', async () => {
    const onChange = vi.fn();
    render(<Tabs tabs={tabs} activeKey="a" onChange={onChange} />);
    await userEvent.click(screen.getByRole('tab', { name: /Beta/ }));
    expect(onChange).toHaveBeenCalledWith('b');
  });

  it('renders badge when provided', () => {
    render(<Tabs tabs={tabs} activeKey="a" onChange={() => undefined} />);
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('navigates with arrow keys', async () => {
    const onChange = vi.fn();
    render(<Tabs tabs={tabs} activeKey="a" onChange={onChange} />);
    const alphaTab = screen.getByRole('tab', { name: /Alpha/ });
    alphaTab.focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(onChange).toHaveBeenCalledWith('b');
  });
});

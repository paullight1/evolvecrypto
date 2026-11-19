import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { EmptyState } from './EmptyState';

describe('EmptyState', () => {
  it('renders title', () => {
    render(<EmptyState title="No orders" />);
    expect(screen.getByText('No orders')).toBeInTheDocument();
  });

  it('renders icon when provided', () => {
    render(<EmptyState title="Empty" icon={<span data-testid="icon">icon</span>} />);
    expect(screen.getByTestId('icon')).toBeInTheDocument();
  });

  it('renders description when provided', () => {
    render(<EmptyState title="Empty" description="No data available." />);
    expect(screen.getByText('No data available.')).toBeInTheDocument();
  });

  it('does not render description when omitted', () => {
    render(<EmptyState title="Empty" />);
    expect(screen.queryByText('No data available.')).not.toBeInTheDocument();
  });

  it('renders action when provided', () => {
    render(
      <EmptyState
        title="Empty"
        action={<button>Place Order</button>}
      />,
    );
    expect(screen.getByRole('button', { name: 'Place Order' })).toBeInTheDocument();
  });

  it('does not render icon wrapper when icon is omitted', () => {
    const { container } = render(<EmptyState title="Empty" />);
    // No icon div rendered — title is the first child element inside root
    expect(container.querySelector('[class*="icon"]')).not.toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(<EmptyState title="Empty" className="my-class" />);
    expect(container.firstChild).toHaveClass('my-class');
  });
});

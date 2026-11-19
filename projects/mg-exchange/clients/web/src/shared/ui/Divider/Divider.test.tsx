import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Divider } from './Divider';

describe('Divider', () => {
  it('renders a horizontal rule by default', () => {
    const { container } = render(<Divider />);
    expect(container.querySelector('hr')).toBeInTheDocument();
  });

  it('renders hr for explicit horizontal orientation', () => {
    const { container } = render(<Divider orientation="horizontal" />);
    expect(container.querySelector('hr')).toBeInTheDocument();
  });

  it('renders a span for vertical orientation', () => {
    const { container } = render(<Divider orientation="vertical" />);
    expect(container.querySelector('hr')).not.toBeInTheDocument();
    expect(container.querySelector('span')).toBeInTheDocument();
  });

  it('shows label text when label prop is provided', () => {
    render(<Divider label="OR" />);
    expect(screen.getByText('OR')).toBeInTheDocument();
  });

  it('does not show label element when label is omitted', () => {
    render(<Divider />);
    expect(screen.queryByRole('separator')).toBeInTheDocument();
    // The <hr> itself is the separator; no text node present
    expect(screen.queryByText(/.+/)).not.toBeInTheDocument();
  });

  it('sets aria-orientation horizontal on hr', () => {
    const { container } = render(<Divider />);
    const hr = container.querySelector('hr');
    expect(hr).toHaveAttribute('aria-orientation', 'horizontal');
  });

  it('sets aria-orientation vertical on vertical divider', () => {
    render(<Divider orientation="vertical" />);
    const separator = screen.getByRole('separator');
    expect(separator).toHaveAttribute('aria-orientation', 'vertical');
  });

  it('renders with sm spacing without throwing', () => {
    const { container } = render(<Divider spacing="sm" />);
    expect(container.querySelector('hr')).toBeInTheDocument();
  });

  it('renders with lg spacing without throwing', () => {
    const { container } = render(<Divider spacing="lg" />);
    expect(container.querySelector('hr')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(<Divider className="my-divider" />);
    expect(container.firstChild).toHaveClass('my-divider');
  });
});

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Card } from './Card';

describe('Card', () => {
  it('renders children', () => {
    render(<Card>Hello</Card>);
    expect(screen.getByText('Hello')).toBeInTheDocument();
  });

  it('renders with no padding', () => {
    const { container } = render(<Card padding="none">Content</Card>);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('renders with sm padding', () => {
    const { container } = render(<Card padding="sm">Content</Card>);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('renders with md padding (default)', () => {
    const { container } = render(<Card>Content</Card>);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(<Card className="my-custom-class">Content</Card>);
    expect(container.firstChild).toHaveClass('my-custom-class');
  });
});

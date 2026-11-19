import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { Button } from './Button';

describe('Button', () => {
  it('renders children', () => {
    render(<Button>Click me</Button>);
    expect(screen.getByRole('button', { name: 'Click me' })).toBeInTheDocument();
  });

  it('calls onClick when clicked', async () => {
    const handler = vi.fn();
    render(<Button onClick={handler}>Click me</Button>);
    await userEvent.click(screen.getByRole('button'));
    expect(handler).toHaveBeenCalledOnce();
  });

  it('is disabled when disabled prop is set', () => {
    render(<Button disabled>Click me</Button>);
    expect(screen.getByRole('button')).toBeDisabled();
  });

  it('does not call onClick when disabled', async () => {
    const handler = vi.fn();
    render(<Button disabled onClick={handler}>Click me</Button>);
    await userEvent.click(screen.getByRole('button'));
    expect(handler).not.toHaveBeenCalled();
  });

  it('shows spinner and is disabled when loading', () => {
    render(<Button loading>Click me</Button>);
    const btn = screen.getByRole('button');
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('renders with primary variant by default', () => {
    render(<Button>Default</Button>);
    expect(screen.getByRole('button')).toBeInTheDocument();
  });

  it('renders long variant', () => {
    render(<Button variant="long">Buy</Button>);
    expect(screen.getByRole('button')).toBeInTheDocument();
  });

  it('renders full width', () => {
    render(<Button fullWidth>Full</Button>);
    expect(screen.getByRole('button')).toBeInTheDocument();
  });
});

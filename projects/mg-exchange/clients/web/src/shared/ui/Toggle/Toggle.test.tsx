import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { Toggle } from './Toggle';

describe('Toggle', () => {
  it('renders unchecked by default', () => {
    render(<Toggle checked={false} onChange={vi.fn()} />);
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'false');
  });

  it('renders checked when checked=true', () => {
    render(<Toggle checked={true} onChange={vi.fn()} />);
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'true');
  });

  it('calls onChange with true when clicking unchecked toggle', async () => {
    const handler = vi.fn();
    render(<Toggle checked={false} onChange={handler} />);
    await userEvent.click(screen.getByRole('switch'));
    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(true);
  });

  it('calls onChange with false when clicking checked toggle', async () => {
    const handler = vi.fn();
    render(<Toggle checked={true} onChange={handler} />);
    await userEvent.click(screen.getByRole('switch'));
    expect(handler).toHaveBeenCalledWith(false);
  });

  it('does not call onChange when disabled', async () => {
    const handler = vi.fn();
    render(<Toggle checked={false} onChange={handler} disabled />);
    await userEvent.click(screen.getByRole('switch'));
    expect(handler).not.toHaveBeenCalled();
  });

  it('is disabled when disabled prop is set', () => {
    render(<Toggle checked={false} onChange={vi.fn()} disabled />);
    expect(screen.getByRole('switch')).toBeDisabled();
  });

  it('shows label when provided', () => {
    render(<Toggle checked={false} onChange={vi.fn()} label="Dark mode" />);
    expect(screen.getByText('Dark mode')).toBeInTheDocument();
  });

  it('shows description when provided', () => {
    render(
      <Toggle
        checked={false}
        onChange={vi.fn()}
        label="Alerts"
        description="Get notified of price changes"
      />,
    );
    expect(screen.getByText('Get notified of price changes')).toBeInTheDocument();
  });

  it('toggles via Space key', async () => {
    const handler = vi.fn();
    render(<Toggle checked={false} onChange={handler} />);
    screen.getByRole('switch').focus();
    await userEvent.keyboard(' ');
    expect(handler).toHaveBeenCalledWith(true);
  });

  it('toggles via Enter key', async () => {
    const handler = vi.fn();
    render(<Toggle checked={false} onChange={handler} />);
    screen.getByRole('switch').focus();
    await userEvent.keyboard('{Enter}');
    expect(handler).toHaveBeenCalledWith(true);
  });

  it('does not toggle via keyboard when disabled', async () => {
    const handler = vi.fn();
    render(<Toggle checked={false} onChange={handler} disabled />);
    screen.getByRole('switch').focus();
    await userEvent.keyboard(' ');
    expect(handler).not.toHaveBeenCalled();
  });

  it('has correct aria attributes', () => {
    render(<Toggle checked={true} onChange={vi.fn()} />);
    const toggle = screen.getByRole('switch');
    expect(toggle).toHaveAttribute('role', 'switch');
    expect(toggle).toHaveAttribute('aria-checked', 'true');
  });
});

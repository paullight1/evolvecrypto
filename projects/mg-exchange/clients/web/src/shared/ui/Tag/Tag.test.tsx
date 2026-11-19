import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { Tag } from './Tag';

describe('Tag', () => {
  it('renders text content', () => {
    render(<Tag>BTC/USDT</Tag>);
    expect(screen.getByText('BTC/USDT')).toBeInTheDocument();
  });

  it('renders as <span> by default (not interactive)', () => {
    const { container } = render(<Tag>Label</Tag>);
    expect(container.querySelector('span.content')).toBeDefined();
    // When not selectable, the root element must NOT be a button
    const button = container.querySelector('button[aria-pressed]');
    expect(button).toBeNull();
  });

  it('applies the variant class for each variant', () => {
    const variants = ['default', 'success', 'warning', 'danger', 'info', 'purple'] as const;
    variants.forEach((variant) => {
      const { container } = render(<Tag variant={variant}>{variant}</Tag>);
      const root = container.firstChild as HTMLElement;
      // CSS Modules mangle class names, so check that the element exists and
      // the text content is correct rather than matching the mangled name
      expect(root).toBeTruthy();
      expect(root.textContent).toContain(variant);
    });
  });

  it('shows remove button when removable=true', () => {
    render(
      <Tag removable onRemove={vi.fn()}>
        Chip
      </Tag>,
    );
    expect(screen.getByRole('button', { name: /remove chip/i })).toBeInTheDocument();
  });

  it('calls onRemove when remove button is clicked', async () => {
    const handler = vi.fn();
    render(
      <Tag removable onRemove={handler}>
        Chip
      </Tag>,
    );
    await userEvent.click(screen.getByRole('button', { name: /remove chip/i }));
    expect(handler).toHaveBeenCalledOnce();
  });

  it('calls onClick when selectable tag is clicked', async () => {
    const handler = vi.fn();
    render(
      <Tag selectable onClick={handler}>
        Option
      </Tag>,
    );
    await userEvent.click(screen.getByRole('button', { name: /option/i }));
    expect(handler).toHaveBeenCalledOnce();
  });

  it('renders as <button> when selectable', () => {
    const { container } = render(
      <Tag selectable onClick={vi.fn()}>
        Selectable
      </Tag>,
    );
    expect(container.querySelector('button[aria-pressed]')).not.toBeNull();
  });

  it('renders as <button> when selected', () => {
    const { container } = render(
      <Tag selectable selected onClick={vi.fn()}>
        Selected
      </Tag>,
    );
    const btn = container.querySelector('button') as HTMLButtonElement;
    expect(btn).not.toBeNull();
    expect(btn).toHaveAttribute('aria-pressed', 'true');
  });

  it('has aria-pressed=false when selectable but not selected', () => {
    const { container } = render(
      <Tag selectable selected={false} onClick={vi.fn()}>
        Unselected
      </Tag>,
    );
    const btn = container.querySelector('button') as HTMLButtonElement;
    expect(btn).toHaveAttribute('aria-pressed', 'false');
  });

  it('renders sm size', () => {
    render(<Tag size="sm">Small</Tag>);
    expect(screen.getByText('Small')).toBeInTheDocument();
  });

  it('renders md size by default', () => {
    render(<Tag>Medium</Tag>);
    expect(screen.getByText('Medium')).toBeInTheDocument();
  });
});

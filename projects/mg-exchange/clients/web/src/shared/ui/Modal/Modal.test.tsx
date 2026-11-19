import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { Modal } from './Modal';

describe('Modal', () => {
  it('does not render when closed', () => {
    render(<Modal open={false} onClose={vi.fn()}>Content</Modal>);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders when open', () => {
    render(<Modal open onClose={vi.fn()}>Content</Modal>);
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Content')).toBeInTheDocument();
  });

  it('renders title when provided', () => {
    render(<Modal open onClose={vi.fn()} title="Confirm Order">Content</Modal>);
    expect(screen.getByText('Confirm Order')).toBeInTheDocument();
  });

  it('calls onClose when close button clicked', async () => {
    const onClose = vi.fn();
    render(<Modal open onClose={onClose} title="Test">Content</Modal>);
    await userEvent.click(screen.getByLabelText('Close modal'));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('calls onClose when overlay is clicked', async () => {
    const onClose = vi.fn();
    render(<Modal open onClose={onClose}>Content</Modal>);
    // Click the overlay (the outermost presentation div)
    const overlay = document.querySelector('[role="presentation"]') as HTMLElement;
    await userEvent.click(overlay);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('calls onClose when Escape is pressed', async () => {
    const onClose = vi.fn();
    render(<Modal open onClose={onClose}>Content</Modal>);
    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('does not call onClose when dialog content is clicked', async () => {
    const onClose = vi.fn();
    render(<Modal open onClose={onClose}>Content</Modal>);
    await userEvent.click(screen.getByText('Content'));
    expect(onClose).not.toHaveBeenCalled();
  });
});

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ConfirmDialog } from './ConfirmDialog';

describe('ConfirmDialog', () => {
  it('does not render when closed', () => {
    render(
      <ConfirmDialog
        open={false}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
        title="Delete"
        message="Are you sure?"
      />,
    );
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders title and message when open', () => {
    render(
      <ConfirmDialog
        open
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
        title="Delete item"
        message="This cannot be undone."
      />,
    );
    expect(screen.getByText('Delete item')).toBeInTheDocument();
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
  });

  it('calls onConfirm when confirm button clicked', async () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={onConfirm}
        onCancel={vi.fn()}
        title="Delete"
        message="Sure?"
      />,
    );
    await userEvent.click(screen.getByText('Confirm'));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it('calls onCancel when cancel button clicked', async () => {
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={vi.fn()}
        onCancel={onCancel}
        title="Delete"
        message="Sure?"
      />,
    );
    await userEvent.click(screen.getByText('Cancel'));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('uses custom confirmText', () => {
    render(
      <ConfirmDialog
        open
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
        title="Confirm Action"
        message="Revoke key?"
        confirmText="Revoke Now"
      />,
    );
    expect(screen.getByRole('button', { name: 'Revoke Now' })).toBeInTheDocument();
  });

  it('calls onCancel when Escape is pressed', async () => {
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        onConfirm={vi.fn()}
        onCancel={onCancel}
        title="Delete"
        message="Sure?"
      />,
    );
    await userEvent.keyboard('{Escape}');
    expect(onCancel).toHaveBeenCalledOnce();
  });
});

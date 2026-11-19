import { render, screen, act, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ToastContainer } from './Toast';
import { toast, useToastStore } from './store';

function resetStore() {
  useToastStore.setState({ toasts: [] });
}

describe('Toast store', () => {
  beforeEach(resetStore);

  it('adds a success toast', () => {
    act(() => { toast.success('Trade filled'); });
    expect(useToastStore.getState().toasts).toHaveLength(1);
    expect(useToastStore.getState().toasts[0].type).toBe('success');
    expect(useToastStore.getState().toasts[0].message).toBe('Trade filled');
  });

  it('adds an error toast', () => {
    act(() => { toast.error('Insufficient balance'); });
    expect(useToastStore.getState().toasts[0].type).toBe('error');
  });

  it('removes toast by id', () => {
    act(() => { toast.success('Hello'); });
    const { id } = useToastStore.getState().toasts[0];
    act(() => { useToastStore.getState().removeToast(id); });
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });
});

describe('ToastContainer', () => {
  beforeEach(() => {
    resetStore();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders nothing when no toasts', () => {
    render(<ToastContainer />);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('renders toast messages', () => {
    act(() => { toast.success('Order placed'); });
    render(<ToastContainer />);
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Order placed')).toBeInTheDocument();
  });

  it('dismisses toast on button click', () => {
    act(() => { toast.error('Error occurred'); });
    render(<ToastContainer />);
    act(() => { fireEvent.click(screen.getByLabelText('Dismiss notification')); });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('auto-dismisses after 4 seconds', () => {
    act(() => { toast.info('Auto dismiss'); });
    render(<ToastContainer />);
    expect(screen.getByRole('alert')).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(4001); });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});

import { render, screen, fireEvent, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { SearchInput } from './SearchInput';

describe('SearchInput', () => {
  it('renders with default placeholder', () => {
    render(<SearchInput value="" onChange={vi.fn()} />);
    expect(screen.getByPlaceholderText('Search...')).toBeInTheDocument();
  });

  it('renders with custom placeholder', () => {
    render(<SearchInput value="" onChange={vi.fn()} placeholder="Search markets…" />);
    expect(screen.getByPlaceholderText('Search markets…')).toBeInTheDocument();
  });

  it('renders the search icon', () => {
    render(<SearchInput value="" onChange={vi.fn()} />);
    // SVG is aria-hidden; verify via container query
    const { container } = render(<SearchInput value="" onChange={vi.fn()} />);
    expect(container.querySelector('svg')).toBeInTheDocument();
  });

  it('fires onChange when input changes', () => {
    const handler = vi.fn();
    render(<SearchInput value="" onChange={handler} />);
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'BTC' } });
    expect(handler).toHaveBeenCalledWith('BTC');
  });

  it('does not show clear button when value is empty', () => {
    render(<SearchInput value="" onChange={vi.fn()} />);
    expect(screen.queryByRole('button', { name: /clear/i })).not.toBeInTheDocument();
  });

  it('shows clear button when value is non-empty', () => {
    render(<SearchInput value="BTC" onChange={vi.fn()} />);
    expect(screen.getByRole('button', { name: /clear/i })).toBeInTheDocument();
  });

  it('clear button calls onChange with empty string', () => {
    const handler = vi.fn();
    render(<SearchInput value="BTC" onChange={handler} />);
    fireEvent.click(screen.getByRole('button', { name: /clear/i }));
    expect(handler).toHaveBeenCalledWith('');
  });

  it('clear button calls onClear callback if provided', () => {
    const onClear = vi.fn();
    render(<SearchInput value="ETH" onChange={vi.fn()} onClear={onClear} />);
    fireEvent.click(screen.getByRole('button', { name: /clear/i }));
    expect(onClear).toHaveBeenCalledOnce();
  });

  it('does not call onClear when it is not provided', () => {
    // Should not throw even when onClear is undefined
    render(<SearchInput value="BTC" onChange={vi.fn()} />);
    expect(() => {
      fireEvent.click(screen.getByRole('button', { name: /clear/i }));
    }).not.toThrow();
  });

  describe('debounce', () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('does not call onChange immediately when debounceMs > 0', async () => {
      const handler = vi.fn();
      render(<SearchInput value="" onChange={handler} debounceMs={300} />);
      const input = screen.getByRole('searchbox');

      fireEvent.change(input, { target: { value: 'B' } });
      expect(handler).not.toHaveBeenCalled();
    });

    it('calls onChange after debounce delay', () => {
      const handler = vi.fn();
      render(<SearchInput value="" onChange={handler} debounceMs={300} />);
      const input = screen.getByRole('searchbox');

      fireEvent.change(input, { target: { value: 'BTC' } });
      expect(handler).not.toHaveBeenCalled();

      act(() => {
        vi.advanceTimersByTime(300);
      });

      expect(handler).toHaveBeenCalledWith('BTC');
    });

    it('resets the debounce timer on rapid changes', () => {
      const handler = vi.fn();
      render(<SearchInput value="" onChange={handler} debounceMs={300} />);
      const input = screen.getByRole('searchbox');

      fireEvent.change(input, { target: { value: 'B' } });
      act(() => { vi.advanceTimersByTime(100); });

      fireEvent.change(input, { target: { value: 'BT' } });
      act(() => { vi.advanceTimersByTime(100); });

      fireEvent.change(input, { target: { value: 'BTC' } });
      // Has not fired yet
      expect(handler).not.toHaveBeenCalled();

      act(() => { vi.advanceTimersByTime(300); });
      // Should have been called exactly once with the final value
      expect(handler).toHaveBeenCalledTimes(1);
      expect(handler).toHaveBeenCalledWith('BTC');
    });
  });
});

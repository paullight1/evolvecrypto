import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NumberInput } from './NumberInput';

describe('NumberInput', () => {
  it('renders with the given value', () => {
    render(<NumberInput value="42" onChange={vi.fn()} placeholder="0.00" />);
    expect(screen.getByDisplayValue('42')).toBeInTheDocument();
  });

  it('renders placeholder when value is empty', () => {
    render(<NumberInput value="" onChange={vi.fn()} placeholder="Enter amount" />);
    expect(screen.getByPlaceholderText('Enter amount')).toBeInTheDocument();
  });

  it('fires onChange with typed numeric input', async () => {
    const handler = vi.fn();
    render(<NumberInput value="" onChange={handler} />);
    await userEvent.type(screen.getByRole('textbox'), '1');
    expect(handler).toHaveBeenCalledWith('1');
  });

  it('ignores non-numeric characters', async () => {
    const handler = vi.fn();
    render(<NumberInput value="" onChange={handler} />);
    await userEvent.type(screen.getByRole('textbox'), 'abc');
    // 'a', 'b', 'c' are non-numeric — handler should not be called
    expect(handler).not.toHaveBeenCalled();
  });

  it('allows decimal input', async () => {
    const handler = vi.fn();
    render(<NumberInput value="1" onChange={handler} />);
    // Simulate typing a dot after "1"
    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: '1.' } });
    expect(handler).toHaveBeenCalledWith('1.');
  });

  it('stepper increment button calls onChange with value + step', () => {
    const handler = vi.fn();
    render(<NumberInput value="5" onChange={handler} step={1} />);
    const incrementBtn = screen.getByRole('button', { name: 'Increment' });
    fireEvent.pointerDown(incrementBtn);
    fireEvent.pointerUp(incrementBtn);
    expect(handler).toHaveBeenCalledWith('6');
  });

  it('stepper decrement button calls onChange with value - step', () => {
    const handler = vi.fn();
    render(<NumberInput value="5" onChange={handler} step={1} />);
    const decrementBtn = screen.getByRole('button', { name: 'Decrement' });
    fireEvent.pointerDown(decrementBtn);
    fireEvent.pointerUp(decrementBtn);
    expect(handler).toHaveBeenCalledWith('4');
  });

  it('clamps to min on increment below min', () => {
    const handler = vi.fn();
    render(<NumberInput value="1" onChange={handler} step={1} min={1} max={10} />);
    const decrementBtn = screen.getByRole('button', { name: 'Decrement' });
    fireEvent.pointerDown(decrementBtn);
    fireEvent.pointerUp(decrementBtn);
    // 1 - 1 = 0 but min is 1, should clamp to 1
    expect(handler).toHaveBeenCalledWith('1');
  });

  it('clamps to max on increment above max', () => {
    const handler = vi.fn();
    render(<NumberInput value="10" onChange={handler} step={1} min={1} max={10} />);
    const incrementBtn = screen.getByRole('button', { name: 'Increment' });
    fireEvent.pointerDown(incrementBtn);
    fireEvent.pointerUp(incrementBtn);
    // 10 + 1 = 11 but max is 10, should clamp to 10
    expect(handler).toHaveBeenCalledWith('10');
  });

  it('clamps value to min on blur when below min', () => {
    const handler = vi.fn();
    render(<NumberInput value="-5" onChange={handler} min={0} />);
    fireEvent.blur(screen.getByRole('textbox'));
    expect(handler).toHaveBeenCalledWith('0');
  });

  it('clamps value to max on blur when above max', () => {
    const handler = vi.fn();
    render(<NumberInput value="999" onChange={handler} max={100} />);
    fireEvent.blur(screen.getByRole('textbox'));
    expect(handler).toHaveBeenCalledWith('100');
  });

  it('rounds to precision on blur', () => {
    const handler = vi.fn();
    render(<NumberInput value="3.14159" onChange={handler} precision={2} />);
    fireEvent.blur(screen.getByRole('textbox'));
    expect(handler).toHaveBeenCalledWith('3.14');
  });

  it('shows error message', () => {
    render(<NumberInput value="" onChange={vi.fn()} error="Value is required" />);
    expect(screen.getByRole('alert')).toHaveTextContent('Value is required');
  });

  it('marks input invalid when error is set', () => {
    render(<NumberInput value="" onChange={vi.fn()} label="Amount" error="Required" />);
    expect(screen.getByRole('textbox')).toHaveAttribute('aria-invalid', 'true');
  });

  it('renders prefix and suffix', () => {
    render(<NumberInput value="" onChange={vi.fn()} prefix="$" suffix="USDT" />);
    expect(screen.getByText('$')).toBeInTheDocument();
    expect(screen.getByText('USDT')).toBeInTheDocument();
  });

  it('does not render stepper when showStepper is false', () => {
    render(<NumberInput value="5" onChange={vi.fn()} showStepper={false} />);
    expect(screen.queryByRole('button', { name: 'Increment' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Decrement' })).not.toBeInTheDocument();
  });

  it('disables input and buttons when disabled', () => {
    render(<NumberInput value="5" onChange={vi.fn()} disabled />);
    expect(screen.getByRole('textbox')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Increment' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Decrement' })).toBeDisabled();
  });

  describe('long-press repeat', () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it('repeats increment after hold', () => {
      const handler = vi.fn();
      render(<NumberInput value="0" onChange={handler} step={1} />);
      const incrementBtn = screen.getByRole('button', { name: 'Increment' });

      fireEvent.pointerDown(incrementBtn);
      // Initial step fires immediately
      expect(handler).toHaveBeenCalledTimes(1);

      // Advance past the 500ms delay
      vi.advanceTimersByTime(500);
      // Advance two repeat intervals
      vi.advanceTimersByTime(200);
      // Should have called at least 2 more times (500ms delay + 2x 100ms intervals)
      expect(handler.mock.calls.length).toBeGreaterThan(2);

      fireEvent.pointerUp(incrementBtn);
    });
  });
});

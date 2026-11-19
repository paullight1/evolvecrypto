import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { Checkbox } from './Checkbox';

describe('Checkbox', () => {
  it('renders unchecked by default', () => {
    render(<Checkbox checked={false} onChange={() => undefined} />);
    expect(screen.getByRole('checkbox')).not.toBeChecked();
  });

  it('renders checked when checked prop is true', () => {
    render(<Checkbox checked={true} onChange={() => undefined} />);
    expect(screen.getByRole('checkbox')).toBeChecked();
  });

  it('fires onChange with toggled value on click', async () => {
    const onChange = vi.fn();
    render(<Checkbox checked={false} onChange={onChange} />);
    await userEvent.click(screen.getByRole('checkbox'));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it('does not fire onChange when disabled', async () => {
    const onChange = vi.fn();
    render(<Checkbox checked={false} onChange={onChange} disabled />);
    await userEvent.click(screen.getByRole('checkbox'));
    expect(onChange).not.toHaveBeenCalled();
  });

  it('shows label when provided', () => {
    render(<Checkbox checked={false} onChange={() => undefined} label="Accept terms" />);
    expect(screen.getByText('Accept terms')).toBeInTheDocument();
  });

  it('shows description when provided', () => {
    render(
      <Checkbox
        checked={false}
        onChange={() => undefined}
        label="Post-only"
        description="Order rejected if it executes immediately."
      />,
    );
    expect(screen.getByText('Order rejected if it executes immediately.')).toBeInTheDocument();
  });

  it('shows error message and sets aria-invalid', () => {
    render(
      <Checkbox
        checked={false}
        onChange={() => undefined}
        error="You must accept the terms"
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('You must accept the terms');
    expect(screen.getByRole('checkbox')).toHaveAttribute('aria-invalid', 'true');
  });

  it('sets aria-checked to "mixed" when indeterminate', () => {
    render(<Checkbox checked={false} onChange={() => undefined} indeterminate />);
    expect(screen.getByRole('checkbox')).toHaveAttribute('aria-checked', 'mixed');
  });
});

import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { Slider } from './Slider';

describe('Slider', () => {
  it('renders with the given value', () => {
    render(<Slider value={42} onChange={vi.fn()} />);
    const input = screen.getByRole('slider');
    expect(input).toHaveValue('42');
  });

  it('renders the label when provided', () => {
    render(<Slider value={50} onChange={vi.fn()} label="Leverage" />);
    expect(screen.getByText('Leverage')).toBeInTheDocument();
  });

  it('renders the value display when showValue is true', () => {
    render(<Slider value={75} onChange={vi.fn()} showValue />);
    expect(screen.getByText('75')).toBeInTheDocument();
  });

  it('formats the value with formatValue', () => {
    render(
      <Slider
        value={10}
        onChange={vi.fn()}
        showValue
        formatValue={(x) => `${x}x`}
      />,
    );
    expect(screen.getByText('10x')).toBeInTheDocument();
  });

  it('fires onChange when slider value changes', () => {
    const handler = vi.fn();
    render(<Slider value={20} onChange={handler} min={0} max={100} />);
    const input = screen.getByRole('slider');
    // fireEvent.change is the correct approach for range inputs in jsdom
    fireEvent.change(input, { target: { value: '21' } });
    expect(handler).toHaveBeenCalledWith(21);
  });

  it('renders mark dots when marks are provided', () => {
    const marks = [
      { value: 0,   label: '0%'   },
      { value: 50,  label: '50%'  },
      { value: 100, label: '100%' },
    ];
    render(<Slider value={50} onChange={vi.fn()} marks={marks} />);
    // Labels should be in the DOM
    expect(screen.getByText('0%')).toBeInTheDocument();
    expect(screen.getByText('50%')).toBeInTheDocument();
    expect(screen.getByText('100%')).toBeInTheDocument();
  });

  it('does not render mark labels when marks have no label property', () => {
    const marks = [{ value: 25 }, { value: 75 }];
    const { container } = render(
      <Slider value={50} onChange={vi.fn()} marks={marks} />,
    );
    // No markLabel spans should appear
    expect(container.querySelector('[class*="markLabel"]')).toBeNull();
  });

  it('is disabled when disabled prop is set', () => {
    render(<Slider value={50} onChange={vi.fn()} disabled />);
    expect(screen.getByRole('slider')).toBeDisabled();
  });

  it('sets correct aria attributes', () => {
    render(
      <Slider
        value={30}
        onChange={vi.fn()}
        min={0}
        max={200}
        formatValue={(x) => `${x} units`}
      />,
    );
    const input = screen.getByRole('slider');
    expect(input).toHaveAttribute('aria-valuemin', '0');
    expect(input).toHaveAttribute('aria-valuemax', '200');
    expect(input).toHaveAttribute('aria-valuenow', '30');
    expect(input).toHaveAttribute('aria-valuetext', '30 units');
  });
});

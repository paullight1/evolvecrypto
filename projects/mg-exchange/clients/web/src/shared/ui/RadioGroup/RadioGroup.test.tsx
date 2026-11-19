import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { RadioGroup } from './RadioGroup';

const options = [
  { value: 'limit', label: 'Limit' },
  { value: 'market', label: 'Market' },
  { value: 'stop', label: 'Stop' },
];

const optionsWithDisabled = [
  { value: 'gtc', label: 'GTC' },
  { value: 'ioc', label: 'IOC' },
  { value: 'fok', label: 'FOK', disabled: true },
];

describe('RadioGroup', () => {
  it('renders all option labels', () => {
    render(
      <RadioGroup name="type" options={options} value="limit" onChange={() => undefined} />,
    );
    expect(screen.getByText('Limit')).toBeInTheDocument();
    expect(screen.getByText('Market')).toBeInTheDocument();
    expect(screen.getByText('Stop')).toBeInTheDocument();
  });

  it('marks the correct option as checked', () => {
    render(
      <RadioGroup name="type" options={options} value="market" onChange={() => undefined} />,
    );
    expect(screen.getByRole('radio', { name: 'Market' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'Limit' })).not.toBeChecked();
  });

  it('fires onChange with the selected value on click', async () => {
    const onChange = vi.fn();
    render(<RadioGroup name="type" options={options} value="limit" onChange={onChange} />);
    await userEvent.click(screen.getByRole('radio', { name: 'Stop' }));
    expect(onChange).toHaveBeenCalledWith('stop');
  });

  it('does not fire onChange for a disabled option', async () => {
    const onChange = vi.fn();
    render(
      <RadioGroup
        name="tif"
        options={optionsWithDisabled}
        value="gtc"
        onChange={onChange}
      />,
    );
    const fok = screen.getByRole('radio', { name: 'FOK' });
    expect(fok).toBeDisabled();
    await userEvent.click(fok);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('navigates options with arrow keys (horizontal)', async () => {
    const onChange = vi.fn();
    render(
      <RadioGroup
        name="type"
        options={options}
        value="limit"
        onChange={onChange}
        direction="horizontal"
      />,
    );
    screen.getByRole('radio', { name: 'Limit' }).focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(onChange).toHaveBeenCalledWith('market');
  });

  it('navigates options with arrow keys (vertical)', async () => {
    const onChange = vi.fn();
    render(
      <RadioGroup
        name="type"
        options={options}
        value="limit"
        onChange={onChange}
        direction="vertical"
      />,
    );
    screen.getByRole('radio', { name: 'Limit' }).focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(onChange).toHaveBeenCalledWith('market');
  });

  it('wraps around at the end with arrow navigation', async () => {
    const onChange = vi.fn();
    render(
      <RadioGroup
        name="type"
        options={options}
        value="stop"
        onChange={onChange}
        direction="horizontal"
      />,
    );
    screen.getByRole('radio', { name: 'Stop' }).focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(onChange).toHaveBeenCalledWith('limit');
  });

  it('renders the button variant with aria-checked', () => {
    render(
      <RadioGroup
        name="tf"
        options={options}
        value="market"
        onChange={() => undefined}
        variant="button"
      />,
    );
    const marketBtn = screen.getByRole('radio', { name: 'Market' });
    expect(marketBtn).toHaveAttribute('aria-checked', 'true');
    const limitBtn = screen.getByRole('radio', { name: 'Limit' });
    expect(limitBtn).toHaveAttribute('aria-checked', 'false');
  });

  it('fires onChange in button variant on click', async () => {
    const onChange = vi.fn();
    render(
      <RadioGroup
        name="tf"
        options={options}
        value="limit"
        onChange={onChange}
        variant="button"
      />,
    );
    await userEvent.click(screen.getByRole('radio', { name: 'Stop' }));
    expect(onChange).toHaveBeenCalledWith('stop');
  });

  it('shows error message', () => {
    render(
      <RadioGroup
        name="type"
        options={options}
        value=""
        onChange={() => undefined}
        error="Please select an option"
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('Please select an option');
  });

  it('renders group label', () => {
    render(
      <RadioGroup
        name="type"
        options={options}
        value="limit"
        onChange={() => undefined}
        label="Order Type"
      />,
    );
    expect(screen.getByText('Order Type')).toBeInTheDocument();
  });
});

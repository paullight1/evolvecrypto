import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Select } from './Select';

const options = [
  { value: 'a', label: 'Apple' },
  { value: 'b', label: 'Banana' },
  { value: 'c', label: 'Cherry' },
];

describe('Select', () => {
  it('renders all options', () => {
    render(<Select options={options} value="a" onChange={() => undefined} />);
    expect(screen.getByRole('combobox')).toBeInTheDocument();
    expect(screen.getByText('Apple')).toBeInTheDocument();
    expect(screen.getByText('Banana')).toBeInTheDocument();
    expect(screen.getByText('Cherry')).toBeInTheDocument();
  });

  it('calls onChange when selecting an option', async () => {
    const onChange = vi.fn();
    render(<Select options={options} value="a" onChange={onChange} />);
    await userEvent.selectOptions(screen.getByRole('combobox'), 'b');
    expect(onChange).toHaveBeenCalled();
  });

  it('renders label when provided', () => {
    render(<Select options={options} value="a" onChange={() => undefined} label="Fruit" />);
    expect(screen.getByText('Fruit')).toBeInTheDocument();
  });
});

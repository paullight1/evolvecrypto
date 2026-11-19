import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Table } from './Table';
import type { ColumnDef } from './Table';

interface Row extends Record<string, unknown> {
  name: string;
  price: string;
}

const columns: ColumnDef<Row>[] = [
  { key: 'name', header: 'Name', sortable: true },
  { key: 'price', header: 'Price', sortable: true, align: 'right' },
];

const data: Row[] = [
  { name: 'Apple', price: '1.50' },
  { name: 'Banana', price: '0.75' },
  { name: 'Cherry', price: '3.00' },
];

describe('Table', () => {
  it('renders column headers', () => {
    render(<Table columns={columns} data={data} />);
    expect(screen.getByText('Name')).toBeInTheDocument();
    expect(screen.getByText('Price')).toBeInTheDocument();
  });

  it('renders all data rows', () => {
    render(<Table columns={columns} data={data} />);
    expect(screen.getByText('Apple')).toBeInTheDocument();
    expect(screen.getByText('Banana')).toBeInTheDocument();
    expect(screen.getByText('Cherry')).toBeInTheDocument();
  });

  it('shows empty state when data is empty', () => {
    render(<Table columns={columns} data={[]} emptyText="Nothing here" />);
    expect(screen.getByText('Nothing here')).toBeInTheDocument();
  });

  it('shows skeleton elements when loading', () => {
    const { container } = render(<Table columns={columns} data={[]} loading />);
    // Skeleton component renders aria-hidden spans
    const skeletons = container.querySelectorAll('[aria-hidden="true"]');
    expect(skeletons.length).toBeGreaterThan(0);
  });

  it('sorts ascending on first click', async () => {
    render(<Table columns={columns} data={data} />);
    await userEvent.click(screen.getByText(/Price/));
    const cells = screen.getAllByRole('cell');
    // After sorting by price asc, Banana (0.75) should come first
    expect(cells[1]).toHaveTextContent('0.75');
  });

  it('calls onRowClick when a row is clicked', async () => {
    const onRowClick = vi.fn();
    render(<Table columns={columns} data={data} onRowClick={onRowClick} />);
    await userEvent.click(screen.getByText('Apple'));
    expect(onRowClick).toHaveBeenCalledWith(data[0]);
  });
});

import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  createAd: vi.fn(),
}));

import { CreateAdForm } from './CreateAdForm';
import { createAd } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';

const mockCreateAd = vi.mocked(createAd);

beforeEach(() => {
  vi.clearAllMocks();
  mockCreateAd.mockResolvedValue(undefined as never);
  useToastStore.setState({ toasts: [] });
});

describe('CreateAdForm', () => {
  it('renders all required fields', async () => {
    await act(async () => { render(<CreateAdForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText(/Price/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Total Amount/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Min Order/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Max Order/)).toBeInTheDocument();
  });

  it('renders payment method checkboxes', async () => {
    await act(async () => { render(<CreateAdForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText(/Bank Transfer/)).toBeInTheDocument();
    expect(screen.getByLabelText(/PayPal/)).toBeInTheDocument();
    expect(screen.getByLabelText(/SEPA/)).toBeInTheDocument();
  });

  it('toggles payment method checkbox', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<CreateAdForm onCreated={vi.fn()} />); });

    const paypalCheckbox = screen.getByRole('checkbox', { name: /PayPal/ });
    expect(paypalCheckbox).not.toBeChecked();

    await user.click(paypalCheckbox);
    expect(paypalCheckbox).toBeChecked();

    await user.click(paypalCheckbox);
    expect(paypalCheckbox).not.toBeChecked();
  });

  it('shows toast error when no payment method is selected', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<CreateAdForm onCreated={vi.fn()} />); });

    // Uncheck the default bank_transfer
    await user.click(screen.getByRole('checkbox', { name: /Bank Transfer/ }));

    // Fill in required fields
    await user.type(screen.getByLabelText(/Price \(USD\)/), '1.01');
    await user.type(screen.getByLabelText(/Total Amount/), '1000');
    await user.type(screen.getByLabelText(/Min Order/), '50');
    await user.type(screen.getByLabelText(/Max Order/), '500');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Post Ad' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error' && t.message.includes('payment method'))).toBe(true);
    });

    expect(mockCreateAd).not.toHaveBeenCalled();
  });

  it('calls createAd and onCreated on valid submit', async () => {
    const user = userEvent.setup();
    const onCreated = vi.fn();
    await act(async () => { render(<CreateAdForm onCreated={onCreated} />); });

    await user.type(screen.getByLabelText(/Price \(USD\)/), '1.01');
    await user.type(screen.getByLabelText(/Total Amount/), '1000');
    await user.type(screen.getByLabelText(/Min Order/), '50');
    await user.type(screen.getByLabelText(/Max Order/), '500');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Post Ad' }));
    });

    await waitFor(() => {
      expect(mockCreateAd).toHaveBeenCalledWith(
        expect.objectContaining({
          price: '1.01',
          min_amount: '50',
          max_amount: '500',
          payment_methods: expect.arrayContaining(['bank_transfer']),
        }),
      );
      expect(onCreated).toHaveBeenCalled();
    });
  });
});

import { describe, test, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import P2PPage from './P2PPage';

vi.mock('./components/AdList', () => ({
  AdList: ({ side }: { side: string }) => <div data-testid={`ad-list-${side}`} />,
}));
vi.mock('./components/OrderFlow', () => ({
  OrderFlow: () => <div data-testid="order-flow" />,
}));
vi.mock('./components/CreateAdForm', () => ({
  CreateAdForm: ({ onCreated }: { onCreated: () => void }) => (
    <div data-testid="create-ad-form" onClick={onCreated} />
  ),
}));

describe('P2PPage', () => {
  test('renders page title', () => {
    render(<P2PPage />);
    expect(screen.getByText('P2P Trading')).toBeInTheDocument();
  });

  test('renders tabs', () => {
    render(<P2PPage />);
    expect(screen.getByText('Buy')).toBeInTheDocument();
    expect(screen.getByText('Sell')).toBeInTheDocument();
    expect(screen.getByText('My Orders')).toBeInTheDocument();
    expect(screen.getByText('My Ads')).toBeInTheDocument();
  });

  test('shows buy ad list by default', () => {
    render(<P2PPage />);
    expect(screen.getByTestId('ad-list-buy')).toBeInTheDocument();
  });

  test('does not render sell list on buy tab', () => {
    render(<P2PPage />);
    expect(screen.queryByTestId('ad-list-sell')).not.toBeInTheDocument();
  });

  test('switching to Sell tab shows sell ad list', async () => {
    render(<P2PPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'Sell' }));
    await waitFor(() => {
      expect(screen.getByTestId('ad-list-sell')).toBeInTheDocument();
      expect(screen.queryByTestId('ad-list-buy')).not.toBeInTheDocument();
    });
  });

  test('switching to My Orders tab shows order flow component', async () => {
    render(<P2PPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'My Orders' }));
    await waitFor(() => {
      expect(screen.getByTestId('order-flow')).toBeInTheDocument();
      expect(screen.queryByTestId('ad-list-buy')).not.toBeInTheDocument();
    });
  });

  test('switching to My Ads tab shows create ad form', async () => {
    render(<P2PPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'My Ads' }));
    await waitFor(() => {
      expect(screen.getByTestId('create-ad-form')).toBeInTheDocument();
    });
  });

  test('Buy and Sell tabs are mutually exclusive', async () => {
    render(<P2PPage />);
    // Start on Buy
    expect(screen.getByTestId('ad-list-buy')).toBeInTheDocument();

    // Switch to Sell
    await userEvent.click(screen.getByRole('tab', { name: 'Sell' }));
    await waitFor(() => expect(screen.queryByTestId('ad-list-buy')).not.toBeInTheDocument());

    // Switch back to Buy
    await userEvent.click(screen.getByRole('tab', { name: 'Buy' }));
    await waitFor(() => {
      expect(screen.getByTestId('ad-list-buy')).toBeInTheDocument();
      expect(screen.queryByTestId('ad-list-sell')).not.toBeInTheDocument();
    });
  });
});

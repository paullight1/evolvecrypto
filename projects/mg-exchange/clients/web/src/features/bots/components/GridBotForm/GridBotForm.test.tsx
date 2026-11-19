import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  createGridBot: vi.fn(),
  getMyBots: vi.fn(),
  createDcaBot: vi.fn(),
  pauseBot: vi.fn(),
  stopBot: vi.fn(),
}));

import { GridBotForm } from './GridBotForm';
import { createGridBot } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';

const mockCreateGridBot = vi.mocked(createGridBot);

beforeEach(() => {
  vi.clearAllMocks();
  mockCreateGridBot.mockResolvedValue({ bot_id: 'bot-1' });
  useToastStore.setState({ toasts: [] });
});

describe('GridBotForm', () => {
  it('renders symbol selector', async () => {
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Symbol')).toBeInTheDocument();
  });

  it('renders upper price input', async () => {
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Upper Price')).toBeInTheDocument();
  });

  it('renders lower price input', async () => {
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Lower Price')).toBeInTheDocument();
  });

  it('renders grid count input', async () => {
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Grid Count')).toBeInTheDocument();
  });

  it('renders investment input', async () => {
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Investment (USDT)')).toBeInTheDocument();
  });

  it('renders Create Grid Bot button', async () => {
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    expect(screen.getByRole('button', { name: 'Create Grid Bot' })).toBeInTheDocument();
  });

  it('shows validation error when upper price <= lower price', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '50000');
    await user.type(screen.getByLabelText('Lower Price'), '60000'); // lower > upper

    await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));

    await waitFor(() => {
      expect(screen.getByText('Upper price must be greater than lower price')).toBeInTheDocument();
    });

    expect(mockCreateGridBot).not.toHaveBeenCalled();
  });

  it('shows grid preview when valid upper/lower prices and investment are set', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.clear(screen.getByLabelText('Upper Price'));
    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.clear(screen.getByLabelText('Lower Price'));
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.clear(screen.getByLabelText('Investment (USDT)'));
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await waitFor(() => {
      expect(screen.getByText('Preview')).toBeInTheDocument();
    });
  });

  it('shows grid spacing in the preview', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    // Grid count default is 10, so spacing = (70000-60000)/(10-1) = 1111.1111
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await waitFor(() => {
      expect(screen.getByText('Grid Spacing')).toBeInTheDocument();
    });
  });

  it('shows per-grid amount in the preview', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await waitFor(() => {
      expect(screen.getByText('Per-Grid Amount')).toBeInTheDocument();
      // 1000 / 10 = 100
      expect(screen.getByText('100.00 USDT')).toBeInTheDocument();
    });
  });

  it('calls createGridBot API with correct params on valid submit', async () => {
    const user = userEvent.setup();
    const onCreated = vi.fn();
    await act(async () => { render(<GridBotForm onCreated={onCreated} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    });

    await waitFor(() => {
      expect(mockCreateGridBot).toHaveBeenCalledWith(
        expect.objectContaining({
          symbol: 'BTCUSDT',
          upper_price: '70000',
          lower_price: '60000',
          grid_count: 10,
          investment: '1000',
        }),
      );
    });
  });

  it('calls onCreated callback after successful bot creation', async () => {
    const user = userEvent.setup();
    const onCreated = vi.fn();
    await act(async () => { render(<GridBotForm onCreated={onCreated} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    });

    await waitFor(() => {
      expect(onCreated).toHaveBeenCalled();
    });
  });

  it('shows success toast after bot created', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'success' && t.message === 'Grid bot created')).toBe(true);
    });
  });

  it('shows validation error when investment is missing', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    // No investment

    await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));

    await waitFor(() => {
      expect(screen.getByText('Enter a valid investment amount')).toBeInTheDocument();
    });

    expect(mockCreateGridBot).not.toHaveBeenCalled();
  });

  // === Price Validation ===

  it('rejects when upper price is below lower price (inverted range)', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    // upper=50000 < lower=60000 — inverted range
    await user.type(screen.getByLabelText('Upper Price'), '50000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));

    await waitFor(() => {
      expect(screen.getByText('Upper price must be greater than lower price')).toBeInTheDocument();
    });
    expect(mockCreateGridBot).not.toHaveBeenCalled();
  });

  it('rejects when upper price equals lower price', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '60000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));

    await waitFor(() => {
      expect(screen.getByText('Upper price must be greater than lower price')).toBeInTheDocument();
    });
    expect(mockCreateGridBot).not.toHaveBeenCalled();
  });

  // === Grid Preview ===

  it('shows correct grid spacing value in preview', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    // Default grid count = 10, spacing = (70000-60000)/(10-1) = 1111.1111
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    await waitFor(() => {
      expect(screen.getByText('1111.1111 USDT')).toBeInTheDocument();
    });
  });

  it('shows correct per-grid investment amount in preview', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '500');
    // Default grid count = 10 → per grid = 500 / 10 = 50.00

    await waitFor(() => {
      expect(screen.getByText('50.00 USDT')).toBeInTheDocument();
    });
  });

  it('preview is not shown when investment is missing', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    // No investment — perGrid returns '-', showPreview = false

    expect(screen.queryByText('Preview')).not.toBeInTheDocument();
  });

  // === Investment Validation ===

  it('rejects zero investment', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '0');

    await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));

    await waitFor(() => {
      expect(screen.getByText('Enter a valid investment amount')).toBeInTheDocument();
    });
    expect(mockCreateGridBot).not.toHaveBeenCalled();
  });

  it('rejects grid count below minimum of 2', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<GridBotForm onCreated={vi.fn()} />); });

    await user.type(screen.getByLabelText('Upper Price'), '70000');
    await user.type(screen.getByLabelText('Lower Price'), '60000');
    await user.type(screen.getByLabelText('Investment (USDT)'), '1000');

    const gridCountInput = screen.getByLabelText('Grid Count');
    await user.clear(gridCountInput);
    await user.type(gridCountInput, '1');

    await user.click(screen.getByRole('button', { name: 'Create Grid Bot' }));

    await waitFor(() => {
      expect(screen.getByText('Grid count must be between 2 and 200')).toBeInTheDocument();
    });
    expect(mockCreateGridBot).not.toHaveBeenCalled();
  });
});

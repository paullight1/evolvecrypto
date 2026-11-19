import type { Meta, StoryObj } from '@storybook/react-vite';
import { ToastContainer } from './Toast';
import { toast } from './store';

const meta = {
  title: 'Shared/Toast',
  component: ToastContainer,
  parameters: {
    layout: 'fullscreen',
  },
  decorators: [
    (Story) => (
      <div style={{ minHeight: '200px', padding: '24px', background: 'var(--bg-1)' }}>
        <Story />
        <ToastContainer />
      </div>
    ),
  ],
} satisfies Meta<typeof ToastContainer>;

export default meta;
type Story = StoryObj<typeof meta>;

export const AllVariants: Story = {
  render: () => (
    <div style={{ display: 'flex', gap: '12px', flexWrap: 'wrap' }}>
      <button
        style={{
          padding: '8px 16px',
          background: 'var(--long)',
          color: '#000',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontFamily: 'var(--font-ui)',
        }}
        onClick={() => toast.success('Order filled: +0.5 BTC at 68,420')}
      >
        Success
      </button>
      <button
        style={{
          padding: '8px 16px',
          background: 'var(--short)',
          color: '#fff',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontFamily: 'var(--font-ui)',
        }}
        onClick={() => toast.error('Insufficient margin to place order')}
      >
        Error
      </button>
      <button
        style={{
          padding: '8px 16px',
          background: 'var(--warning)',
          color: '#000',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontFamily: 'var(--font-ui)',
        }}
        onClick={() => toast.warning('Position approaching liquidation price')}
      >
        Warning
      </button>
      <button
        style={{
          padding: '8px 16px',
          background: 'var(--info)',
          color: '#fff',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontFamily: 'var(--font-ui)',
        }}
        onClick={() => toast.info('Funding rate payment in 2 minutes')}
      >
        Info
      </button>
    </div>
  ),
};

export const Success: Story = {
  render: () => (
    <button
      style={{
        padding: '8px 16px',
        background: 'var(--long)',
        color: '#000',
        border: 'none',
        borderRadius: 'var(--radius-sm)',
        cursor: 'pointer',
        fontFamily: 'var(--font-ui)',
      }}
      onClick={() => toast.success('Trade executed successfully')}
    >
      Fire success toast
    </button>
  ),
};

export const Error: Story = {
  render: () => (
    <button
      style={{
        padding: '8px 16px',
        background: 'var(--short)',
        color: '#fff',
        border: 'none',
        borderRadius: 'var(--radius-sm)',
        cursor: 'pointer',
        fontFamily: 'var(--font-ui)',
      }}
      onClick={() => toast.error('Network error: failed to connect to exchange')}
    >
      Fire error toast
    </button>
  ),
};

export const Warning: Story = {
  render: () => (
    <button
      style={{
        padding: '8px 16px',
        background: 'var(--warning)',
        color: '#000',
        border: 'none',
        borderRadius: 'var(--radius-sm)',
        cursor: 'pointer',
        fontFamily: 'var(--font-ui)',
      }}
      onClick={() => toast.warning('High leverage detected: 50x on BTCUSDT')}
    >
      Fire warning toast
    </button>
  ),
};

export const Info: Story = {
  render: () => (
    <button
      style={{
        padding: '8px 16px',
        background: 'var(--info)',
        color: '#fff',
        border: 'none',
        borderRadius: 'var(--radius-sm)',
        cursor: 'pointer',
        fontFamily: 'var(--font-ui)',
      }}
      onClick={() => toast.info('Maintenance window scheduled for 03:00 UTC')}
    >
      Fire info toast
    </button>
  ),
};

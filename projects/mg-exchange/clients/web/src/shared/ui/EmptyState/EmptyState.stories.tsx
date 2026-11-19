import type { Meta, StoryObj } from '@storybook/react-vite';
import { EmptyState } from './EmptyState';

const InboxIcon = () => (
  <svg viewBox="0 0 48 48" fill="none" xmlns="http://www.w3.org/2000/svg">
    <rect x="6" y="10" width="36" height="28" rx="3" stroke="currentColor" strokeWidth="2" />
    <path d="M6 28h9l3 4h12l3-4h9" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
  </svg>
);

const meta = {
  title: 'Shared/UI/EmptyState',
  component: EmptyState,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '480px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    title: 'No data',
  },
} satisfies Meta<typeof EmptyState>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    title: 'No data',
  },
};

export const WithIcon: Story = {
  args: {
    icon: <InboxIcon />,
    title: 'No orders yet',
  },
};

export const WithDescription: Story = {
  args: {
    icon: <InboxIcon />,
    title: 'No open orders',
    description: 'You have no open orders. Place an order to get started.',
  },
};

export const WithAction: Story = {
  args: {
    icon: <InboxIcon />,
    title: 'No open positions',
    description: 'You have no open positions. Start trading to see your positions here.',
    action: (
      <button
        style={{
          padding: '6px 16px',
          background: 'var(--accent)',
          color: '#fff',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontSize: 'var(--text-sm)',
          fontWeight: 'var(--font-semibold)',
        }}
      >
        Start Trading
      </button>
    ),
  },
};

export const InTable: Story = {
  name: 'In Table',
  decorators: [
    () => (
      <div
        style={{
          width: '640px',
          border: '1px solid var(--border-1)',
          borderRadius: 'var(--radius)',
          background: 'var(--bg-2)',
        }}
      >
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: '1fr 1fr 1fr',
            padding: '8px 16px',
            borderBottom: '1px solid var(--border-1)',
            fontSize: 'var(--text-xs)',
            color: 'var(--text-2)',
            fontFamily: 'var(--font-ui)',
          }}
        >
          <span>Symbol</span>
          <span>Side</span>
          <span>Amount</span>
        </div>
        <EmptyState title="No trade history" description="Completed trades will appear here." />
      </div>
    ),
  ],
  args: { title: '' },
};

export const Minimal: Story = {
  args: {
    title: 'Nothing here',
  },
};

import type { Meta, StoryObj } from '@storybook/react-vite';
import { Tooltip } from './Tooltip';

const meta = {
  title: 'Shared/Tooltip',
  component: Tooltip,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ padding: '80px 40px', background: 'var(--bg-1)' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Tooltip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Top: Story = {
  args: {
    content: 'Unrealized PnL as percentage of initial margin',
    side: 'top',
    children: <span style={{ cursor: 'default', color: 'var(--text-1)', fontFamily: 'var(--font-ui)' }}>ROE% ⓘ</span>,
  },
};

export const Bottom: Story = {
  args: {
    content: 'Mark price is derived from the index price + funding basis',
    side: 'bottom',
    children: <span style={{ cursor: 'default', color: 'var(--text-1)', fontFamily: 'var(--font-ui)' }}>Mark Price ⓘ</span>,
  },
};

export const OnButton: Story = {
  args: {
    content: 'Transfers available balance to your futures wallet',
    side: 'top',
    children: (
      <button
        style={{
          padding: '6px 14px',
          background: 'var(--accent)',
          color: 'var(--on-accent)',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontFamily: 'var(--font-ui)',
          fontSize: 'var(--text-sm)',
        }}
      >
        Transfer
      </button>
    ),
  },
};

export const LongContent: Story = {
  args: {
    content: 'ADL rank is calculated as profit ratio × leverage, descending. High rank means you are at risk of auto-deleveraging.',
    side: 'top',
    children: <span style={{ cursor: 'default', color: 'var(--warning)', fontFamily: 'var(--font-ui)' }}>ADL Rank ⓘ</span>,
  },
};

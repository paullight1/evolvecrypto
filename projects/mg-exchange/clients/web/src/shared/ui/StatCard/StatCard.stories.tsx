import type { Meta, StoryObj } from '@storybook/react-vite';
import { StatCard } from './StatCard';

const meta = {
  title: 'Shared/UI/StatCard',
  component: StatCard,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '240px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    label: 'Total Equity',
    value: '12,450.00',
  },
} satisfies Meta<typeof StatCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    label: 'Total Equity',
    value: '12,450.00',
  },
};

export const WithChange: Story = {
  name: 'With Change (up)',
  args: {
    label: '24h PnL',
    value: '340.50',
    change: '+2.81%',
    trend: 'up',
  },
};

export const WithNegativeChange: Story = {
  name: 'With Change (down)',
  args: {
    label: '24h PnL',
    value: '-120.00',
    change: '-0.96%',
    trend: 'down',
  },
};

export const WithIcon: Story = {
  args: {
    label: 'Open Positions',
    value: '4',
    icon: (
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
        <path d="M8 2v12M2 8h12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
  },
};

export const WithPrefixSuffix: Story = {
  args: {
    label: 'Available Balance',
    value: '8,200.00',
    prefix: '$',
    suffix: 'USDT',
  },
};

export const Loading: Story = {
  args: {
    label: 'Total Equity',
    value: '0',
    loading: true,
  },
};

export const Grid: Story = {
  name: 'Grid (4 cards)',
  decorators: [
    () => (
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(4, 200px)',
          gap: '12px',
        }}
      >
        <StatCard label="Total Equity" value="12,450.00" prefix="$" />
        <StatCard label="Available" value="8,200.00" prefix="$" />
        <StatCard label="24h PnL" value="+340.50" change="+2.81%" trend="up" />
        <StatCard label="Margin Used" value="4,250.00" prefix="$" change="-1.2%" trend="down" />
      </div>
    ),
  ],
  args: {
    label: '',
    value: '',
  },
};

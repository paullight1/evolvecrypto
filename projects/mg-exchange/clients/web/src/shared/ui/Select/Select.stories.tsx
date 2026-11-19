import type { Meta, StoryObj } from '@storybook/react-vite';
import { Select } from './Select';

const TIMEFRAME_OPTIONS = [
  { value: '1m', label: '1 Minute' },
  { value: '5m', label: '5 Minutes' },
  { value: '15m', label: '15 Minutes' },
  { value: '1h', label: '1 Hour' },
  { value: '4h', label: '4 Hours' },
  { value: '1d', label: '1 Day' },
];

const ORDER_TYPE_OPTIONS = [
  { value: 'limit', label: 'Limit' },
  { value: 'market', label: 'Market' },
  { value: 'stop_limit', label: 'Stop Limit' },
  { value: 'stop_market', label: 'Stop Market' },
];

const meta = {
  title: 'Shared/UI/Select',
  component: Select,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '200px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    options: ORDER_TYPE_OPTIONS,
  },
} satisfies Meta<typeof Select>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    options: ORDER_TYPE_OPTIONS,
    size: 'md',
  },
};

export const WithLabel: Story = {
  args: {
    label: 'Order Type',
    options: ORDER_TYPE_OPTIONS,
    size: 'md',
  },
};

export const SizeMd: Story = {
  name: 'Size: md',
  args: {
    label: 'Timeframe',
    options: TIMEFRAME_OPTIONS,
    size: 'md',
  },
};

export const SizeSm: Story = {
  name: 'Size: sm',
  args: {
    label: 'Timeframe',
    options: TIMEFRAME_OPTIONS,
    size: 'sm',
  },
};

export const Disabled: Story = {
  args: {
    label: 'Settlement',
    options: [{ value: 'usdt', label: 'USDT Linear' }],
    disabled: true,
  },
};

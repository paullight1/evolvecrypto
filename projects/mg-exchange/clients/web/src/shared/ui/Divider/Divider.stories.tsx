import type { Meta, StoryObj } from '@storybook/react-vite';
import { Divider } from './Divider';

const meta = {
  title: 'Shared/UI/Divider',
  component: Divider,
  parameters: {
    layout: 'padded',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '400px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    orientation: 'horizontal',
    spacing: 'md',
  },
} satisfies Meta<typeof Divider>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Horizontal: Story = {
  name: 'Horizontal (default)',
  args: {
    orientation: 'horizontal',
    spacing: 'md',
  },
  decorators: [
    () => (
      <div style={{ width: '400px' }}>
        <p style={{ margin: 0, color: 'var(--text-1)', fontSize: 'var(--text-sm)' }}>Section A</p>
        <Divider />
        <p style={{ margin: 0, color: 'var(--text-1)', fontSize: 'var(--text-sm)' }}>Section B</p>
      </div>
    ),
  ],
};

export const WithLabel: Story = {
  args: {
    label: 'OR',
    spacing: 'md',
  },
};

export const Vertical: Story = {
  name: 'Vertical',
  decorators: [
    () => (
      <div style={{ display: 'flex', alignItems: 'center', height: '32px', gap: 0 }}>
        <span style={{ color: 'var(--text-1)', fontSize: 'var(--text-sm)' }}>BTC/USDT</span>
        <Divider orientation="vertical" spacing="sm" />
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-sm)' }}>Perpetual</span>
        <Divider orientation="vertical" spacing="sm" />
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-sm)' }}>100x</span>
      </div>
    ),
  ],
  args: { orientation: 'vertical', spacing: 'sm' },
};

export const SmallSpacing: Story = {
  name: 'Spacing: sm',
  args: {
    spacing: 'sm',
  },
  decorators: [
    () => (
      <div style={{ width: '400px' }}>
        <p style={{ margin: 0, fontSize: 'var(--text-sm)', color: 'var(--text-1)' }}>Above</p>
        <Divider spacing="sm" />
        <p style={{ margin: 0, fontSize: 'var(--text-sm)', color: 'var(--text-1)' }}>Below</p>
      </div>
    ),
  ],
};

export const LargeSpacing: Story = {
  name: 'Spacing: lg',
  args: {
    spacing: 'lg',
  },
  decorators: [
    () => (
      <div style={{ width: '400px' }}>
        <p style={{ margin: 0, fontSize: 'var(--text-sm)', color: 'var(--text-1)' }}>Above</p>
        <Divider spacing="lg" />
        <p style={{ margin: 0, fontSize: 'var(--text-sm)', color: 'var(--text-1)' }}>Below</p>
      </div>
    ),
  ],
};

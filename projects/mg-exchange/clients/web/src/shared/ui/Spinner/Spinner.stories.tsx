import type { Meta, StoryObj } from '@storybook/react-vite';
import { Spinner } from './Spinner';

const meta = {
  title: 'Shared/Spinner',
  component: Spinner,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ padding: '32px', background: 'var(--bg-1)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Spinner>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Small: Story = {
  args: {
    size: 'sm',
  },
};

export const Medium: Story = {
  args: {
    size: 'md',
  },
};

export const Large: Story = {
  args: {
    size: 'lg',
  },
};

export const AllSizes: Story = {
  render: () => (
    <div style={{ display: 'flex', gap: '32px', alignItems: 'center', padding: '32px', background: 'var(--bg-1)' }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '8px' }}>
        <Spinner size="sm" />
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-xs)', fontFamily: 'var(--font-ui)' }}>sm</span>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '8px' }}>
        <Spinner size="md" />
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-xs)', fontFamily: 'var(--font-ui)' }}>md</span>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '8px' }}>
        <Spinner size="lg" />
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-xs)', fontFamily: 'var(--font-ui)' }}>lg</span>
      </div>
    </div>
  ),
};

export const InlineWithText: Story = {
  render: () => (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px', color: 'var(--text-1)', fontFamily: 'var(--font-ui)', fontSize: 'var(--text-sm)', background: 'var(--bg-1)', padding: '16px' }}>
      <Spinner size="sm" />
      <span>Loading order book...</span>
    </div>
  ),
};

import type { Meta, StoryObj } from '@storybook/react-vite';
import { Skeleton } from './Skeleton';

const meta = {
  title: 'Shared/Skeleton',
  component: Skeleton,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ padding: '32px', background: 'var(--bg-1)', minWidth: '320px' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Skeleton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const TextLine: Story = {
  args: {
    variant: 'text',
    width: '200px',
  },
};

export const TextMultiLine: Story = {
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', width: '280px' }}>
      <Skeleton variant="text" width="80%" />
      <Skeleton variant="text" width="100%" />
      <Skeleton variant="text" width="65%" />
    </div>
  ),
};

export const Rect: Story = {
  args: {
    variant: 'rect',
    width: '100%',
    height: '120px',
  },
};

export const Circle: Story = {
  args: {
    variant: 'circle',
    width: '48px',
    height: '48px',
  },
};

export const PositionCardSkeleton: Story = {
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', width: '320px', padding: '16px', background: 'var(--bg-2)', borderRadius: 'var(--radius)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
        <Skeleton variant="circle" width="40px" height="40px" />
        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', flex: 1 }}>
          <Skeleton variant="text" width="60%" />
          <Skeleton variant="text" width="40%" />
        </div>
      </div>
      <Skeleton variant="rect" width="100%" height="8px" />
      <div style={{ display: 'flex', gap: '8px' }}>
        <Skeleton variant="rect" width="50%" height="40px" />
        <Skeleton variant="rect" width="50%" height="40px" />
      </div>
    </div>
  ),
};

export const OrderBookSkeleton: Story = {
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '4px', width: '280px' }}>
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} style={{ display: 'flex', gap: '8px', justifyContent: 'space-between' }}>
          <Skeleton variant="text" width="80px" />
          <Skeleton variant="text" width="80px" />
          <Skeleton variant="text" width="80px" />
        </div>
      ))}
    </div>
  ),
};

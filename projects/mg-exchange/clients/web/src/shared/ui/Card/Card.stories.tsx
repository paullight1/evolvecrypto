import type { Meta, StoryObj } from '@storybook/react-vite';
import { Card } from './Card';

const meta = {
  title: 'Shared/UI/Card',
  component: Card,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '320px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    children: (
      <div>
        <p style={{ margin: 0, fontWeight: 600 }}>BTC/USDT Perpetual</p>
        <p style={{ margin: '4px 0 0', color: 'var(--color-text-secondary)', fontSize: '14px' }}>
          Mark Price: $67,450.00
        </p>
      </div>
    ),
    padding: 'md',
  },
} satisfies Meta<typeof Card>;

export default meta;
type Story = StoryObj<typeof meta>;

export const PaddingMd: Story = {
  name: 'Padding: md (default)',
  args: { padding: 'md' },
};

export const PaddingSm: Story = {
  name: 'Padding: sm',
  args: { padding: 'sm' },
};

export const PaddingNone: Story = {
  name: 'Padding: none',
  args: {
    padding: 'none',
    children: (
      <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--color-border)' }}>
        <p style={{ margin: 0, fontWeight: 600 }}>Header (custom padding)</p>
      </div>
    ),
  },
};

export const WithComplexContent: Story = {
  name: 'With complex content',
  args: {
    padding: 'md',
    children: (
      <div>
        <p style={{ margin: '0 0 12px', fontWeight: 600, fontSize: '16px' }}>Account Summary</p>
        {[
          { label: 'Equity',      value: '$12,450.00' },
          { label: 'Available',   value: '$8,200.00'  },
          { label: 'Unrealised PnL', value: '+$340.00' },
          { label: 'Margin Used', value: '$4,250.00'  },
        ].map(({ label, value }) => (
          <div key={label} style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '8px', fontSize: '14px' }}>
            <span style={{ color: 'var(--color-text-secondary)' }}>{label}</span>
            <span>{value}</span>
          </div>
        ))}
      </div>
    ),
  },
};

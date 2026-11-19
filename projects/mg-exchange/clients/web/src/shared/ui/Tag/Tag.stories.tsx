import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Tag, type TagVariant } from './Tag';

const meta = {
  title: 'Shared/UI/Tag',
  component: Tag,
  parameters: {
    layout: 'centered',
  },
  args: {
    children: 'Tag',
    variant: 'default',
    size: 'md',
  },
} satisfies Meta<typeof Tag>;

export default meta;
type Story = StoryObj<typeof meta>;

// --- Basic ---

export const Default: Story = {
  args: { children: 'BTC/USDT' },
};

// --- All variants ---

export const AllVariants: Story = {
  name: 'All Variants',
  render: () => {
    const variants: TagVariant[] = ['default', 'success', 'warning', 'danger', 'info', 'purple'];
    return (
      <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap', alignItems: 'center' }}>
        {variants.map((v) => (
          <Tag key={v} variant={v}>
            {v}
          </Tag>
        ))}
      </div>
    );
  },
};

// --- Small size ---

export const SmallSize: Story = {
  name: 'Size: sm',
  render: () => {
    const variants: TagVariant[] = ['default', 'success', 'warning', 'danger', 'info', 'purple'];
    return (
      <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap', alignItems: 'center' }}>
        {variants.map((v) => (
          <Tag key={v} variant={v} size="sm">
            {v}
          </Tag>
        ))}
      </div>
    );
  },
};

// --- Removable ---

export const Removable: Story = {
  render: () => {
    const [visible, setVisible] = useState(true);
    if (!visible) {
      return (
        <button type="button" onClick={() => setVisible(true)} style={{ fontSize: '12px' }}>
          Restore tag
        </button>
      );
    }
    return (
      <Tag variant="info" removable onRemove={() => setVisible(false)}>
        BTC/USDT
      </Tag>
    );
  },
};

// --- Selectable ---

export const Selectable: Story = {
  render: () => {
    const [sel, setSel] = useState(false);
    return (
      <Tag selectable selected={sel} onClick={() => setSel((s) => !s)}>
        {sel ? 'Selected' : 'Click me'}
      </Tag>
    );
  },
};

// --- Selected (static) ---

export const Selected: Story = {
  args: {
    children: 'USDT',
    selectable: true,
    selected: true,
  },
};

// --- Tag group (multi-select filter) ---

export const TagGroup: Story = {
  name: 'Tag Group (filter)',
  render: () => {
    const coins = ['BTC', 'ETH', 'SOL', 'BNB', 'XRP'];
    const [active, setActive] = useState<Set<string>>(new Set(['BTC', 'ETH']));
    function toggle(coin: string) {
      setActive((prev) => {
        const next = new Set(prev);
        next.has(coin) ? next.delete(coin) : next.add(coin);
        return next;
      });
    }
    return (
      <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
        {coins.map((coin) => (
          <Tag
            key={coin}
            selectable
            selected={active.has(coin)}
            onClick={() => toggle(coin)}
          >
            {coin}
          </Tag>
        ))}
      </div>
    );
  },
};

// --- Long text ---

export const WithLongText: Story = {
  args: {
    children: 'Perpetual Futures Contract',
    variant: 'info',
  },
};

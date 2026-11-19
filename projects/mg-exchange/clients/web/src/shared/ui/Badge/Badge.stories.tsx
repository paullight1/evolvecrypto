import type { Meta, StoryObj } from '@storybook/react-vite';
import { Badge, type BadgeVariant, type BadgeSize } from './Badge';

const meta = {
  title: 'Shared/UI/Badge',
  component: Badge,
  parameters: {
    layout: 'centered',
  },
  args: {
    children: 'Badge',
    variant: 'neutral',
    size: 'md',
  },
} satisfies Meta<typeof Badge>;

export default meta;
type Story = StoryObj<typeof meta>;

// --- Individual variants ---

export const Success: Story = {
  args: { variant: 'success', children: 'Active' },
};

export const Danger: Story = {
  args: { variant: 'danger', children: 'Liquidated' },
};

export const Warning: Story = {
  args: { variant: 'warning', children: 'At Risk' },
};

export const Info: Story = {
  args: { variant: 'info', children: 'Pending' },
};

export const Neutral: Story = {
  args: { variant: 'neutral', children: 'Closed' },
};

export const Long: Story = {
  args: { variant: 'long', children: 'Long' },
};

export const Short: Story = {
  args: { variant: 'short', children: 'Short' },
};

// --- Sizes ---

export const SizeMd: Story = {
  name: 'Size: md',
  args: { variant: 'success', size: 'md', children: 'Medium' },
};

export const SizeSm: Story = {
  name: 'Size: sm',
  args: { variant: 'success', size: 'sm', children: 'Small' },
};

// --- All variants grid ---

export const AllVariants: Story = {
  name: 'All Variants × Sizes',
  render: () => {
    const variants: BadgeVariant[] = ['success', 'danger', 'warning', 'info', 'neutral', 'long', 'short'];
    const sizes: BadgeSize[] = ['md', 'sm'];
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
        {sizes.map((size) => (
          <div key={size} style={{ display: 'flex', gap: '8px', alignItems: 'center', flexWrap: 'wrap' }}>
            <span style={{ width: '30px', fontSize: '12px', color: 'var(--color-text-secondary)' }}>{size}</span>
            {variants.map((variant) => (
              <Badge key={variant} variant={variant} size={size}>
                {variant}
              </Badge>
            ))}
          </div>
        ))}
      </div>
    );
  },
};

import type { Meta, StoryObj } from '@storybook/react-vite';
import { Button } from './Button';

const meta = {
  title: 'Shared/UI/Button',
  component: Button,
  parameters: {
    layout: 'centered',
  },
  args: {
    children: 'Button',
    variant: 'primary',
    size: 'md',
    loading: false,
    fullWidth: false,
    disabled: false,
  },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

// --- Variants ---

export const Primary: Story = {
  args: { variant: 'primary', children: 'Primary' },
};

export const Secondary: Story = {
  args: { variant: 'secondary', children: 'Secondary' },
};

export const Ghost: Story = {
  args: { variant: 'ghost', children: 'Ghost' },
};

export const Long: Story = {
  args: { variant: 'long', children: 'Long / Buy' },
};

export const Short: Story = {
  args: { variant: 'short', children: 'Short / Sell' },
};

export const Danger: Story = {
  args: { variant: 'danger', children: 'Danger' },
};

// --- Sizes ---

export const SizeSmall: Story = {
  name: 'Size: sm',
  args: { size: 'sm', children: 'Small' },
};

export const SizeMedium: Story = {
  name: 'Size: md',
  args: { size: 'md', children: 'Medium' },
};

export const SizeLarge: Story = {
  name: 'Size: lg',
  args: { size: 'lg', children: 'Large' },
};

// --- States ---

export const Loading: Story = {
  args: { loading: true, children: 'Submitting' },
};

export const Disabled: Story = {
  args: { disabled: true, children: 'Disabled' },
};

export const FullWidth: Story = {
  args: { fullWidth: true, children: 'Full Width' },
  parameters: { layout: 'padded' },
};

// --- All variants grid ---

export const AllVariants: Story = {
  name: 'All Variants × Sizes',
  render: () => (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, auto)', gap: '12px', alignItems: 'center' }}>
      {(['primary', 'secondary', 'ghost', 'long', 'short', 'danger'] as const).flatMap((variant) =>
        (['sm', 'md', 'lg'] as const).map((size) => (
          <Button key={`${variant}-${size}`} variant={variant} size={size}>
            {variant} {size}
          </Button>
        )),
      )}
    </div>
  ),
};

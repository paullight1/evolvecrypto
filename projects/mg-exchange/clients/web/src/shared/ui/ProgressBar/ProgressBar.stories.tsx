import type { Meta, StoryObj } from '@storybook/react-vite';
import { ProgressBar } from './ProgressBar';

const meta = {
  title: 'Shared/UI/ProgressBar',
  component: ProgressBar,
  parameters: {
    layout: 'padded',
  },
  args: {
    value: 65,
    max: 100,
    variant: 'accent',
    size: 'md',
    showValue: false,
  },
  decorators: [
    (Story) => (
      <div style={{ width: 320 }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof ProgressBar>;

export default meta;
type Story = StoryObj<typeof meta>;

// --- Fill levels ---

export const Default: Story = {
  args: { value: 65 },
};

export const HalfFilled: Story = {
  args: { value: 50 },
};

export const Full: Story = {
  args: { value: 100 },
};

export const Empty: Story = {
  args: { value: 0 },
};

// --- Variants ---

export const LongVariant: Story = {
  name: 'Variant: long',
  args: { value: 72, variant: 'long' },
};

export const ShortVariant: Story = {
  name: 'Variant: short',
  args: { value: 38, variant: 'short' },
};

export const WarningVariant: Story = {
  name: 'Variant: warning',
  args: { value: 88, variant: 'warning' },
};

export const AccentVariant: Story = {
  name: 'Variant: accent',
  args: { value: 55, variant: 'accent' },
};

// --- Sizes ---

export const SmallSize: Story = {
  name: 'Size: sm',
  args: { value: 65, size: 'sm' },
};

// --- With metadata ---

export const WithLabel: Story = {
  args: { value: 65, label: 'Margin usage' },
};

export const WithValue: Story = {
  args: { value: 65, showValue: true },
};

export const WithLabelAndValue: Story = {
  args: { value: 65, label: 'Margin usage', showValue: true },
};

// --- All variants grid ---

export const AllVariants: Story = {
  name: 'All Variants',
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', width: 320 }}>
      {(['accent', 'long', 'short', 'warning', 'default'] as const).map((variant) => (
        <ProgressBar
          key={variant}
          value={65}
          variant={variant}
          label={variant}
          showValue
        />
      ))}
    </div>
  ),
};

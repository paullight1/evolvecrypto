import type { Meta, StoryObj } from '@storybook/react-vite';
import { Input } from './Input';

const meta = {
  title: 'Shared/UI/Input',
  component: Input,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '280px' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Input>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    placeholder: 'Enter value…',
  },
};

export const WithLabel: Story = {
  args: {
    label: 'Order Price',
    placeholder: '0.00',
    type: 'number',
  },
};

export const WithError: Story = {
  args: {
    label: 'Amount',
    placeholder: '0.00',
    value: '-5',
    error: 'Amount must be greater than zero',
    readOnly: true,
  },
};

export const WithPrefix: Story = {
  args: {
    label: 'Price',
    placeholder: '0.00',
    prefix: '$',
    type: 'number',
  },
};

export const WithSuffix: Story = {
  args: {
    label: 'Quantity',
    placeholder: '0.000',
    suffix: 'BTC',
    type: 'number',
  },
};

export const WithPrefixAndSuffix: Story = {
  name: 'With Prefix + Suffix',
  args: {
    label: 'Notional',
    placeholder: '0.00',
    prefix: '$',
    suffix: 'USD',
    type: 'number',
  },
};

export const Disabled: Story = {
  args: {
    label: 'Read-only field',
    value: '67,450.00',
    disabled: true,
    readOnly: true,
  },
};

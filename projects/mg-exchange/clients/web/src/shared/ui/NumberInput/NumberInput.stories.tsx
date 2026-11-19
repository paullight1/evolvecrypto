// @ts-nocheck
import type { Meta, StoryObj } from '@storybook/react-vite';
import { useState } from 'react';
import { NumberInput } from './NumberInput';

const meta = {
  title: 'Shared/UI/NumberInput',
  component: NumberInput,
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
} satisfies Meta<typeof NumberInput>;

export default meta;
type Story = StoryObj<typeof meta>;

// Controlled wrapper for interactive stories
function Controlled(props: Omit<React.ComponentProps<typeof NumberInput>, 'value' | 'onChange'> & { initialValue?: string }) {
  const { initialValue = '', ...rest } = props;
  const [value, setValue] = useState(initialValue);
  return <NumberInput {...rest} value={value} onChange={setValue} />;
}

export const Default: Story = {
  render: () => <Controlled placeholder="0.00" />,
};

export const WithLabel: Story = {
  render: () => <Controlled label="Order Size" placeholder="0.000" initialValue="1" />,
};

export const WithMinMax: Story = {
  render: () => (
    <Controlled
      label="Quantity (1–100)"
      placeholder="1"
      initialValue="50"
      min={1}
      max={100}
    />
  ),
};

export const WithStep: Story = {
  render: () => (
    <Controlled
      label="Leverage (step 5)"
      placeholder="1"
      initialValue="10"
      min={1}
      max={100}
      step={5}
    />
  ),
};

export const WithPrecision: Story = {
  render: () => (
    <Controlled
      label="Price"
      placeholder="0.00000"
      initialValue="67450.50"
      precision={5}
    />
  ),
};

export const WithStepper: Story = {
  render: () => (
    <Controlled
      label="Amount"
      placeholder="0"
      initialValue="1"
      showStepper
    />
  ),
};

export const WithPrefixSuffix: Story = {
  render: () => (
    <Controlled
      label="Notional"
      placeholder="0.00"
      initialValue="1000"
      prefix="$"
      suffix="USDT"
    />
  ),
};

export const WithError: Story = {
  render: () => (
    <Controlled
      label="Amount"
      placeholder="0.000"
      initialValue="0"
      min={0.001}
      error="Amount must be greater than 0"
    />
  ),
};

export const SmallSize: Story = {
  render: () => (
    <Controlled
      label="Qty"
      placeholder="0.00"
      initialValue="1"
      size="sm"
    />
  ),
};

export const Disabled: Story = {
  args: {
    value: '67450.00',
    onChange: () => undefined,
    label: 'Mark Price',
    disabled: true,
  },
};

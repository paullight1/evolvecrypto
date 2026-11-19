// @ts-nocheck
import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { RadioGroup } from './RadioGroup';
import type { RadioGroupProps } from './RadioGroup';

const ORDER_TYPES = [
  { value: 'limit', label: 'Limit' },
  { value: 'market', label: 'Market' },
  { value: 'stop', label: 'Stop' },
];

const TIMEFRAMES = [
  { value: '1m', label: '1m' },
  { value: '5m', label: '5m' },
  { value: '15m', label: '15m' },
  { value: '1h', label: '1h' },
  { value: '4h', label: '4h' },
];

const SIDES = [
  {
    value: 'long',
    label: 'Long',
    description: 'Profit when price rises.',
  },
  {
    value: 'short',
    label: 'Short',
    description: 'Profit when price falls.',
  },
];

const OPTIONS_WITH_DISABLED = [
  { value: 'gtc', label: 'GTC' },
  { value: 'ioc', label: 'IOC' },
  { value: 'fok', label: 'FOK', disabled: true },
];

const meta = {
  title: 'Shared/UI/RadioGroup',
  component: RadioGroup,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ minWidth: '300px' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof RadioGroup>;

export default meta;
type Story = StoryObj<typeof meta>;

function Controlled(props: Omit<RadioGroupProps, 'value' | 'onChange'>) {
  const [val, setVal] = useState(props.options[0].value);
  return <RadioGroup {...props} value={val} onChange={setVal} />;
}

export const Default: Story = {
  render: () => (
    <Controlled name="order-type" options={ORDER_TYPES} />
  ),
};

export const ButtonVariant: Story = {
  render: () => (
    <Controlled name="timeframe" options={TIMEFRAMES} variant="button" />
  ),
};

export const Vertical: Story = {
  render: () => (
    <Controlled name="side" options={SIDES} direction="vertical" />
  ),
};

export const WithLabel: Story = {
  render: () => (
    <Controlled
      name="tif"
      label="Time in Force"
      options={ORDER_TYPES}
    />
  ),
};

export const WithError: Story = {
  render: () => (
    <RadioGroup
      name="required-choice"
      options={ORDER_TYPES}
      value=""
      onChange={() => undefined}
      label="Order Type"
      error="Please select an order type."
    />
  ),
};

export const DisabledOption: Story = {
  render: () => (
    <Controlled
      name="tif-disabled"
      label="Time in Force"
      options={OPTIONS_WITH_DISABLED}
    />
  ),
};

export const SmallSize: Story = {
  render: () => (
    <Controlled
      name="timeframe-sm"
      options={TIMEFRAMES}
      variant="button"
      size="sm"
    />
  ),
};

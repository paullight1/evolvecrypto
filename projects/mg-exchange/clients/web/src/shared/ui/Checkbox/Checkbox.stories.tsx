// @ts-nocheck
import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Checkbox } from './Checkbox';

const meta = {
  title: 'Shared/UI/Checkbox',
  component: Checkbox,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ minWidth: '240px' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Checkbox>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    checked: false,
    onChange: () => undefined,
  },
};

export const Checked: Story = {
  args: {
    checked: true,
    onChange: () => undefined,
  },
};

export const Indeterminate: Story = {
  args: {
    checked: false,
    indeterminate: true,
    onChange: () => undefined,
  },
};

export const WithLabel: Story = {
  args: {
    checked: false,
    label: 'Reduce-only order',
    onChange: () => undefined,
  },
};

export const WithDescription: Story = {
  args: {
    checked: true,
    label: 'Post-only',
    description: 'Order will be rejected if it would execute immediately.',
    onChange: () => undefined,
  },
};

export const WithError: Story = {
  args: {
    checked: false,
    label: 'I agree to the terms',
    error: 'You must accept the terms to continue.',
    onChange: () => undefined,
  },
};

export const Disabled: Story = {
  args: {
    checked: false,
    label: 'Unavailable option',
    disabled: true,
    onChange: () => undefined,
  },
};

function GroupDemo() {
  const [values, setValues] = useState({ spot: true, perp: false, options: false });

  function toggle(key: keyof typeof values) {
    setValues((prev) => ({ ...prev, [key]: !prev[key] }));
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
      <Checkbox checked={values.spot} onChange={() => toggle('spot')} label="Spot" />
      <Checkbox checked={values.perp} onChange={() => toggle('perp')} label="Perpetual Futures" />
      <Checkbox
        checked={values.options}
        onChange={() => toggle('options')}
        label="Options"
        description="Available for verified accounts only."
      />
    </div>
  );
}

export const Group: Story = {
  render: () => <GroupDemo />,
};

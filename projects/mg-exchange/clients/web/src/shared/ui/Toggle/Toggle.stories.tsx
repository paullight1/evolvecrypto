// @ts-nocheck
import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Toggle } from './Toggle';

const meta = {
  title: 'Shared/UI/Toggle',
  component: Toggle,
  parameters: {
    layout: 'centered',
  },
  args: {
    checked: false,
    size: 'md',
    disabled: false,
  },
} satisfies Meta<typeof Toggle>;

export default meta;
type Story = StoryObj<typeof meta>;

// --- Basic states ---

export const Default: Story = {
  args: { checked: false },
};

export const Checked: Story = {
  args: { checked: true },
};

export const Disabled: Story = {
  args: { checked: false, disabled: true },
};

export const DisabledChecked: Story = {
  name: 'Disabled (checked)',
  args: { checked: true, disabled: true },
};

// --- With label ---

export const WithLabel: Story = {
  args: {
    checked: false,
    label: 'Enable notifications',
  },
  parameters: { layout: 'padded' },
  decorators: [
    (Story) => (
      <div style={{ width: 320 }}>
        <Story />
      </div>
    ),
  ],
};

export const WithDescription: Story = {
  args: {
    checked: true,
    label: 'Trade confirmations',
    description: 'Show a confirmation dialog before placing orders',
  },
  parameters: { layout: 'padded' },
  decorators: [
    (Story) => (
      <div style={{ width: 320 }}>
        <Story />
      </div>
    ),
  ],
};

// --- Sizes ---

export const SmallSize: Story = {
  name: 'Size: sm',
  args: { checked: false, size: 'sm' },
};

export const SmallSizeChecked: Story = {
  name: 'Size: sm (checked)',
  args: { checked: true, size: 'sm' },
};

// --- All states grid ---

export const AllStates: Story = {
  name: 'All States',
  render: () => {
    const [states, setStates] = useState({
      default: false,
      withLabel: true,
      withDesc: false,
      small: true,
    });

    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '24px', width: 320 }}>
        <Toggle
          checked={states.default}
          onChange={(v) => setStates((s) => ({ ...s, default: v }))}
        />
        <Toggle
          checked={states.withLabel}
          onChange={(v) => setStates((s) => ({ ...s, withLabel: v }))}
          label="Dark mode"
        />
        <Toggle
          checked={states.withDesc}
          onChange={(v) => setStates((s) => ({ ...s, withDesc: v }))}
          label="Price alerts"
          description="Notify me when price targets are hit"
        />
        <Toggle
          checked={states.small}
          onChange={(v) => setStates((s) => ({ ...s, small: v }))}
          size="sm"
          label="Compact rows"
        />
        <Toggle checked={false} onChange={() => {}} disabled label="Feature unavailable" />
      </div>
    );
  },
};

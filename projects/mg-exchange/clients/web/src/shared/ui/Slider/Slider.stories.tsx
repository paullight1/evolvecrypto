// @ts-nocheck
import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Slider } from './Slider';

const meta = {
  title: 'Shared/UI/Slider',
  component: Slider,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: 320 }}>
        <Story />
      </div>
    ),
  ],
  args: {
    value: 50,
    min: 0,
    max: 100,
    step: 1,
  },
} satisfies Meta<typeof Slider>;

export default meta;
type Story = StoryObj<typeof meta>;

// --- Basic ---

export const Default: Story = {
  render: (args) => {
    const [v, setV] = useState(args.value);
    return <Slider {...args} value={v} onChange={setV} />;
  },
};

// --- With marks (leverage preset) ---

export const WithMarks: Story = {
  name: 'With Marks (leverage)',
  render: () => {
    const marks = [
      { value: 1,   label: '1x'  },
      { value: 2,   label: '2x'  },
      { value: 5,   label: '5x'  },
      { value: 10,  label: '10x' },
      { value: 25,  label: '25x' },
      { value: 50,  label: '50x' },
      { value: 75,  label: '75x' },
      { value: 100, label: '100x'},
    ];
    const [v, setV] = useState(10);
    return (
      <Slider
        value={v}
        onChange={setV}
        min={1}
        max={100}
        step={1}
        marks={marks}
        label="Leverage"
        showValue
        formatValue={(x) => `${x}x`}
      />
    );
  },
};

// --- With label ---

export const WithLabel: Story = {
  render: () => {
    const [v, setV] = useState(30);
    return <Slider value={v} onChange={setV} label="Position size" />;
  },
};

// --- With value display ---

export const WithValue: Story = {
  render: () => {
    const [v, setV] = useState(65);
    return <Slider value={v} onChange={setV} label="Allocation" showValue />;
  },
};

// --- Custom format ---

export const WithFormatValue: Story = {
  name: 'With FormatValue (x => `${x}x`)',
  render: () => {
    const [v, setV] = useState(5);
    return (
      <Slider
        value={v}
        onChange={setV}
        min={1}
        max={20}
        step={1}
        label="Leverage"
        showValue
        formatValue={(x) => `${x}x`}
      />
    );
  },
};

// --- Disabled ---

export const Disabled: Story = {
  render: () => (
    <Slider value={40} onChange={() => {}} label="Disabled" disabled />
  ),
};

// --- Min / Max / Step ---

export const MinMax: Story = {
  name: 'Min=0.1 Max=10 Step=0.1',
  render: () => {
    const [v, setV] = useState(5);
    return (
      <Slider
        value={v}
        onChange={setV}
        min={0.1}
        max={10}
        step={0.1}
        label="Risk multiplier"
        showValue
        formatValue={(x) => x.toFixed(1)}
      />
    );
  },
};

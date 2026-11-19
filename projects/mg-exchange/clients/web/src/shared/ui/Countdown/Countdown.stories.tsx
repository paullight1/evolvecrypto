import type { Meta, StoryObj } from '@storybook/react-vite';
import { Countdown } from './Countdown';

const meta = {
  title: 'Shared/UI/Countdown',
  component: Countdown,
  parameters: {
    layout: 'centered',
  },
} satisfies Meta<typeof Countdown>;

export default meta;
type Story = StoryObj<typeof meta>;

export const FiveMinutes: Story = {
  name: 'Five Minutes',
  args: {
    target: Date.now() + 5 * 60 * 1000,
    format: 'ms',
  },
};

export const OneHour: Story = {
  name: 'One Hour',
  args: {
    target: Date.now() + 60 * 60 * 1000,
    format: 'hms',
  },
};

export const OneDay: Story = {
  name: 'One Day',
  args: {
    target: Date.now() + 24 * 60 * 60 * 1000,
    format: 'dhms',
  },
};

export const Completed: Story = {
  name: 'Completed (target in the past)',
  args: {
    target: Date.now() - 1000,
    completedText: 'Ended',
  },
};

export const WithPrefix: Story = {
  name: 'With Prefix',
  args: {
    target: Date.now() + 90 * 60 * 1000,
    prefix: 'Ends in',
    format: 'hms',
  },
};

export const AutoFormat: Story = {
  name: 'Auto Format (>1 day)',
  args: {
    target: Date.now() + 26 * 60 * 60 * 1000,
    format: 'auto',
  },
};

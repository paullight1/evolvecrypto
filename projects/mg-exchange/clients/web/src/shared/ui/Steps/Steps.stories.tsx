import type { Meta, StoryObj } from '@storybook/react-vite';
import { Steps, type StepDef } from './Steps';

const meta = {
  title: 'Shared/UI/Steps',
  component: Steps,
  parameters: {
    layout: 'padded',
  },
  args: {
    currentStep: 0,
    direction: 'horizontal',
  },
} satisfies Meta<typeof Steps>;

export default meta;
type Story = StoryObj<typeof meta>;

const KYC_STEPS: StepDef[] = [
  { key: 'identity', label: 'Identity' },
  { key: 'address',  label: 'Address' },
  { key: 'review',   label: 'Review' },
];

const DEPOSIT_STEPS: StepDef[] = [
  { key: 'select',   label: 'Select coin',   description: 'Choose network' },
  { key: 'copy',     label: 'Copy address',  description: 'Scan QR or copy' },
  { key: 'confirm',  label: 'Confirm',       description: 'Blockchain confirmation' },
];

export const ThreeSteps: Story = {
  name: 'Three steps (current=1)',
  args: {
    steps: KYC_STEPS,
    currentStep: 1,
  },
};

export const FirstStep: Story = {
  name: 'First step active',
  args: {
    steps: KYC_STEPS,
    currentStep: 0,
  },
};

export const LastStep: Story = {
  name: 'Last step active',
  args: {
    steps: KYC_STEPS,
    currentStep: 2,
  },
};

export const AllCompleted: Story = {
  name: 'All completed (currentStep past end)',
  args: {
    steps: KYC_STEPS,
    currentStep: 3,
  },
};

export const WithDescriptions: Story = {
  name: 'With descriptions (current=1)',
  args: {
    steps: DEPOSIT_STEPS,
    currentStep: 1,
  },
};

export const Vertical: Story = {
  name: 'Vertical (current=1)',
  args: {
    steps: DEPOSIT_STEPS,
    currentStep: 1,
    direction: 'vertical',
  },
  parameters: {
    layout: 'padded',
  },
};

export const VerticalAllCompleted: Story = {
  name: 'Vertical — all completed',
  args: {
    steps: DEPOSIT_STEPS,
    currentStep: 3,
    direction: 'vertical',
  },
};

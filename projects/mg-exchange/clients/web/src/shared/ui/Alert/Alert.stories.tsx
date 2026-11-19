import type { Meta, StoryObj } from '@storybook/react-vite';
import { Alert } from './Alert';

const meta = {
  title: 'Shared/UI/Alert',
  component: Alert,
  parameters: {
    layout: 'padded',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '480px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    variant: 'info',
    children: 'This is an informational message.',
  },
} satisfies Meta<typeof Alert>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Info: Story = {
  args: {
    variant: 'info',
    children: 'Your session will expire in 5 minutes.',
  },
};

export const Warning: Story = {
  args: {
    variant: 'warning',
    children: 'Your margin ratio is approaching the liquidation threshold.',
  },
};

export const Error: Story = {
  args: {
    variant: 'error',
    children: 'Order submission failed. Insufficient available balance.',
  },
};

export const Success: Story = {
  args: {
    variant: 'success',
    children: 'Withdrawal request submitted. Funds will arrive within 30 minutes.',
  },
};

export const WithTitle: Story = {
  args: {
    variant: 'warning',
    title: 'Margin Call',
    children: 'Your position is at risk of liquidation. Add margin or reduce your position size.',
  },
};

export const Dismissible: Story = {
  args: {
    variant: 'info',
    title: 'New feature available',
    children: 'Portfolio margin mode is now available for eligible accounts.',
    dismissible: true,
    onDismiss: () => { /* handled by parent */ },
  },
};

export const WithAction: Story = {
  args: {
    variant: 'warning',
    title: 'Verification required',
    children: 'Complete identity verification to increase your withdrawal limits.',
    action: (
      <a
        href="#"
        style={{
          color: 'var(--accent)',
          fontSize: 'var(--text-sm)',
          fontWeight: 'var(--font-semibold)',
          textDecoration: 'none',
        }}
      >
        Verify identity
      </a>
    ),
  },
};

export const LongContent: Story = {
  args: {
    variant: 'info',
    title: 'System maintenance scheduled',
    children:
      'Scheduled maintenance is planned for 2026-03-25 02:00–04:00 UTC. ' +
      'During this window, order placement will be unavailable. ' +
      'All open orders will remain on the book. ' +
      'Please plan your trading activity accordingly.',
  },
};

export const AllVariants: Story = {
  name: 'All Variants',
  decorators: [
    () => (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', width: '480px' }}>
        <Alert variant="info">Informational message.</Alert>
        <Alert variant="success" title="Success">Order placed successfully.</Alert>
        <Alert variant="warning" title="Warning">Margin ratio is low.</Alert>
        <Alert variant="error" title="Error">Order rejected: insufficient funds.</Alert>
      </div>
    ),
  ],
  args: { variant: 'info', children: '' },
};

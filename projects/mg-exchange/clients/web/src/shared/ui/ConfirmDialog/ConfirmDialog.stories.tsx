import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { ConfirmDialog } from './ConfirmDialog';

const meta = {
  title: 'Shared/ConfirmDialog',
  component: ConfirmDialog,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ padding: '32px', background: 'var(--bg-1)', minHeight: '200px' }}>
        <Story />
      </div>
    ),
  ],
  // Default args satisfy required props for the StoryObj type checker.
  // Individual render-override stories provide their own state via hooks.
  args: {
    open: false,
    title: '',
    message: '',
    onConfirm: () => {},
    onCancel: () => {},
  },
} satisfies Meta<typeof ConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Danger: Story = {
  render: () => {
    const [open, setOpen] = useState(false);
    const [result, setResult] = useState<string>('');

    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', alignItems: 'center' }}>
        <button
          style={{
            padding: '8px 16px',
            background: 'var(--short)',
            color: '#fff',
            border: 'none',
            borderRadius: 'var(--radius-sm)',
            cursor: 'pointer',
            fontFamily: 'var(--font-ui)',
            fontSize: 'var(--text-sm)',
          }}
          onClick={() => setOpen(true)}
        >
          Close All Positions
        </button>
        {result && (
          <span style={{ color: 'var(--text-2)', fontFamily: 'var(--font-ui)', fontSize: 'var(--text-sm)' }}>
            {result}
          </span>
        )}
        <ConfirmDialog
          open={open}
          variant="danger"
          title="Close All Positions"
          message="This will immediately close all 3 open positions at market price. This action cannot be undone."
          confirmText="Close All"
          onConfirm={() => { setResult('Confirmed: positions closed'); setOpen(false); }}
          onCancel={() => { setResult('Cancelled'); setOpen(false); }}
        />
      </div>
    );
  },
};

export const Warning: Story = {
  render: () => {
    const [open, setOpen] = useState(false);
    const [result, setResult] = useState<string>('');

    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', alignItems: 'center' }}>
        <button
          style={{
            padding: '8px 16px',
            background: 'var(--warning)',
            color: '#000',
            border: 'none',
            borderRadius: 'var(--radius-sm)',
            cursor: 'pointer',
            fontFamily: 'var(--font-ui)',
            fontSize: 'var(--text-sm)',
          }}
          onClick={() => setOpen(true)}
        >
          Cancel Order
        </button>
        {result && (
          <span style={{ color: 'var(--text-2)', fontFamily: 'var(--font-ui)', fontSize: 'var(--text-sm)' }}>
            {result}
          </span>
        )}
        <ConfirmDialog
          open={open}
          variant="warning"
          title="Cancel Order"
          message="Cancel limit order to buy 0.5 BTC at $67,800? Any partial fill will be retained."
          confirmText="Cancel Order"
          onConfirm={() => { setResult('Confirmed: order cancelled'); setOpen(false); }}
          onCancel={() => { setResult('Cancelled'); setOpen(false); }}
        />
      </div>
    );
  },
};

export const OpenByDefault: Story = {
  render: () => (
    <ConfirmDialog
      open={true}
      variant="danger"
      title="Revoke API Key"
      message="Revoking this API key will immediately disable any bots or integrations using it. Are you sure?"
      confirmText="Revoke Key"
      onConfirm={() => {}}
      onCancel={() => {}}
    />
  ),
};

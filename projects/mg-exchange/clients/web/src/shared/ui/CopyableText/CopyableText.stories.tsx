import type { Meta, StoryObj } from '@storybook/react-vite';
import { CopyableText } from './CopyableText';

const meta = {
  title: 'Shared/CopyableText',
  component: CopyableText,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ padding: '32px', background: 'var(--bg-1)' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    value: '',
  },
} satisfies Meta<typeof CopyableText>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    value: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
  },
};

export const CustomDisplayValue: Story = {
  args: {
    value: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
    displayValue: '0xd8dA...6045',
  },
};

export const ApiKey: Story = {
  args: {
    value: 'mgx_a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6',
    displayValue: 'mgx_a1b2c3d4••••••••••••••••••••••••o5p6',
  },
};

export const TxHash: Story = {
  args: {
    value: 'f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16',
    displayValue: 'f4184f...1e9e16',
  },
};

export const UidValue: Story = {
  args: {
    value: '12345678',
  },
};

export const InContext: Story = {
  render: () => (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      gap: '12px',
      padding: '16px',
      background: 'var(--bg-2)',
      borderRadius: 'var(--radius)',
      width: '360px',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-sm)', fontFamily: 'var(--font-ui)' }}>UID</span>
        <CopyableText value="87654321" />
      </div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-sm)', fontFamily: 'var(--font-ui)' }}>Deposit Address</span>
        <CopyableText
          value="bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"
          displayValue="bc1qar0...5mdq"
        />
      </div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span style={{ color: 'var(--text-2)', fontSize: 'var(--text-sm)', fontFamily: 'var(--font-ui)' }}>Referral Code</span>
        <CopyableText value="XCHG2026" />
      </div>
    </div>
  ),
};

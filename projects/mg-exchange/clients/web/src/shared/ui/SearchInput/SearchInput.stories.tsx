// @ts-nocheck
import type { Meta, StoryObj } from '@storybook/react-vite';
import { useState } from 'react';
import { SearchInput } from './SearchInput';

const meta = {
  title: 'Shared/UI/SearchInput',
  component: SearchInput,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ width: '280px' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof SearchInput>;

export default meta;
type Story = StoryObj<typeof meta>;

function Controlled(props: Omit<React.ComponentProps<typeof SearchInput>, 'value' | 'onChange'> & { initialValue?: string }) {
  const { initialValue = '', ...rest } = props;
  const [value, setValue] = useState(initialValue);
  return <SearchInput {...rest} value={value} onChange={setValue} />;
}

export const Default: Story = {
  render: () => <Controlled />,
};

export const WithValue: Story = {
  render: () => <Controlled initialValue="BTCUSDT" />,
};

export const WithPlaceholder: Story = {
  render: () => <Controlled placeholder="Search markets…" />,
};

export const SmallSize: Story = {
  render: () => <Controlled size="sm" placeholder="Search…" />,
};

export const AutoFocus: Story = {
  render: () => <Controlled autoFocus placeholder="Focused on mount" />,
};

export const WithClearButton: Story = {
  render: () => {
    const [value, setValue] = useState('ETH');
    return (
      <SearchInput
        value={value}
        onChange={setValue}
        onClear={() => console.log('cleared')}
        placeholder="Search…"
      />
    );
  },
};

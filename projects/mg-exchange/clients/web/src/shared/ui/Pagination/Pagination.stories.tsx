import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Pagination } from './Pagination';

const meta = {
  title: 'Shared/Pagination',
  component: Pagination,
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
    page: 1,
    totalPages: 10,
    onChange: () => {},
  },
} satisfies Meta<typeof Pagination>;

export default meta;
type Story = StoryObj<typeof meta>;

export const FirstPage: Story = {
  args: {
    page: 1,
    totalPages: 10,
  },
};

export const MiddlePage: Story = {
  args: {
    page: 5,
    totalPages: 10,
  },
};

export const LastPage: Story = {
  args: {
    page: 10,
    totalPages: 10,
  },
};

export const SinglePage: Story = {
  args: {
    page: 1,
    totalPages: 1,
  },
};

export const Interactive: Story = {
  render: () => {
    const [page, setPage] = useState(1);
    const totalPages = 8;

    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', alignItems: 'center' }}>
        <div style={{
          padding: '12px 20px',
          background: 'var(--bg-2)',
          borderRadius: 'var(--radius)',
          color: 'var(--text-1)',
          fontFamily: 'var(--font-ui)',
          fontSize: 'var(--text-sm)',
          minWidth: '240px',
          textAlign: 'center',
        }}>
          Showing trades {(page - 1) * 20 + 1}–{Math.min(page * 20, 148)} of 148
        </div>
        <Pagination page={page} totalPages={totalPages} onChange={setPage} />
      </div>
    );
  },
};

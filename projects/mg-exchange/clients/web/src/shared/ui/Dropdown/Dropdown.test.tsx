import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Dropdown, DropdownItem, DropdownSeparator } from './Dropdown';

function BasicDropdown({
  onOpenChange,
}: {
  onOpenChange?: (open: boolean) => void;
}) {
  return (
    <Dropdown trigger={<button>Open</button>} onOpenChange={onOpenChange}>
      <DropdownItem>First</DropdownItem>
      <DropdownItem>Second</DropdownItem>
      <DropdownItem>Third</DropdownItem>
    </Dropdown>
  );
}

describe('Dropdown', () => {
  it('does not show the menu on initial render', () => {
    render(<BasicDropdown />);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('opens on trigger click', async () => {
    render(<BasicDropdown />);
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(screen.getByRole('menu')).toBeInTheDocument();
    expect(screen.getByText('First')).toBeInTheDocument();
  });

  it('closes when clicking outside', async () => {
    render(
      <div>
        <BasicDropdown />
        <div data-testid="outside">Outside</div>
      </div>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(screen.getByRole('menu')).toBeInTheDocument();
    await userEvent.click(screen.getByTestId('outside'));
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('closes on Escape key', async () => {
    render(<BasicDropdown />);
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(screen.getByRole('menu')).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('renders menu items in a portal attached to document.body', async () => {
    const { baseElement } = render(
      <div id="app-root">
        <BasicDropdown />
      </div>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    const menu = screen.getByRole('menu');
    // Portal renders directly under body, not inside #app-root
    expect(baseElement).toContainElement(menu);
    expect(document.getElementById('app-root')).not.toContainElement(menu);
  });

  it('calls onOpenChange(true) when opening', async () => {
    const onOpenChange = vi.fn();
    render(<BasicDropdown onOpenChange={onOpenChange} />);
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });

  it('calls onOpenChange(false) when closing via Escape', async () => {
    const onOpenChange = vi.fn();
    render(<BasicDropdown onOpenChange={onOpenChange} />);
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    onOpenChange.mockClear();
    await userEvent.keyboard('{Escape}');
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('respects controlled open prop', async () => {
    const { rerender } = render(
      <Dropdown open={false} trigger={<button>T</button>}>
        <DropdownItem>Item</DropdownItem>
      </Dropdown>,
    );
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    rerender(
      <Dropdown open={true} trigger={<button>T</button>}>
        <DropdownItem>Item</DropdownItem>
      </Dropdown>,
    );
    expect(screen.getByRole('menu')).toBeInTheDocument();
  });
});

describe('DropdownItem', () => {
  it('fires onClick when clicked', async () => {
    const onClick = vi.fn();
    render(
      <Dropdown trigger={<button>Open</button>}>
        <DropdownItem onClick={onClick}>Click me</DropdownItem>
      </Dropdown>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Click me' }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('does not fire onClick when disabled', async () => {
    const onClick = vi.fn();
    render(
      <Dropdown trigger={<button>Open</button>}>
        <DropdownItem onClick={onClick} disabled>
          Disabled item
        </DropdownItem>
      </Dropdown>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    // disabled button — userEvent won't fire click on a disabled element
    const item = screen.getByRole('menuitem', { name: 'Disabled item' });
    expect(item).toBeDisabled();
    await userEvent.click(item, { pointerEventsCheck: 0 });
    expect(onClick).not.toHaveBeenCalled();
  });

  it('applies destructive class on destructive prop', async () => {
    render(
      <Dropdown trigger={<button>Open</button>}>
        <DropdownItem destructive>Delete</DropdownItem>
      </Dropdown>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    const item = screen.getByRole('menuitem', { name: 'Delete' });
    expect(item.className).toMatch(/destructive/);
  });

  it('renders icon when provided', async () => {
    render(
      <Dropdown trigger={<button>Open</button>}>
        <DropdownItem icon={<span data-testid="icon">★</span>}>With icon</DropdownItem>
      </Dropdown>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(screen.getByTestId('icon')).toBeInTheDocument();
  });
});

describe('DropdownSeparator', () => {
  it('renders a separator element', async () => {
    render(
      <Dropdown trigger={<button>Open</button>}>
        <DropdownItem>Above</DropdownItem>
        <DropdownSeparator />
        <DropdownItem>Below</DropdownItem>
      </Dropdown>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    // <hr> has implicit role "separator"
    expect(screen.getByRole('separator')).toBeInTheDocument();
  });
});

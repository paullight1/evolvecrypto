import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Accordion } from './Accordion';
import type { AccordionItem } from './Accordion';

const items: AccordionItem[] = [
  { key: 'a', title: 'Item A', content: 'Content A' },
  { key: 'b', title: 'Item B', content: 'Content B' },
  { key: 'c', title: 'Item C', content: 'Content C' },
];

const disabledItems: AccordionItem[] = [
  { key: 'a', title: 'Item A', content: 'Content A' },
  { key: 'locked', title: 'Locked', content: 'Secret', disabled: true },
];

describe('Accordion', () => {
  it('renders all item titles', () => {
    render(<Accordion items={items} />);
    expect(screen.getByText('Item A')).toBeInTheDocument();
    expect(screen.getByText('Item B')).toBeInTheDocument();
    expect(screen.getByText('Item C')).toBeInTheDocument();
  });

  it('all items start closed by default', () => {
    render(<Accordion items={items} />);
    const buttons = screen.getAllByRole('button');
    buttons.forEach((btn) => {
      expect(btn).toHaveAttribute('aria-expanded', 'false');
    });
  });

  it('opens an item when its header is clicked', async () => {
    render(<Accordion items={items} />);
    const btnA = screen.getByRole('button', { name: /Item A/ });
    await userEvent.click(btnA);
    expect(btnA).toHaveAttribute('aria-expanded', 'true');
  });

  it('closes an open item when its header is clicked again', async () => {
    render(<Accordion items={items} defaultOpenKeys={['a']} />);
    const btnA = screen.getByRole('button', { name: /Item A/ });
    expect(btnA).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(btnA);
    expect(btnA).toHaveAttribute('aria-expanded', 'false');
  });

  it('single mode: opening one item closes the previously open item', async () => {
    render(<Accordion items={items} defaultOpenKeys={['a']} multiple={false} />);
    const btnB = screen.getByRole('button', { name: /Item B/ });
    await userEvent.click(btnB);
    expect(screen.getByRole('button', { name: /Item A/ })).toHaveAttribute('aria-expanded', 'false');
    expect(btnB).toHaveAttribute('aria-expanded', 'true');
  });

  it('multiple mode: opening one item does not close others', async () => {
    render(<Accordion items={items} defaultOpenKeys={['a']} multiple />);
    const btnB = screen.getByRole('button', { name: /Item B/ });
    await userEvent.click(btnB);
    expect(screen.getByRole('button', { name: /Item A/ })).toHaveAttribute('aria-expanded', 'true');
    expect(btnB).toHaveAttribute('aria-expanded', 'true');
  });

  it('respects defaultOpenKeys', () => {
    render(<Accordion items={items} defaultOpenKeys={['b', 'c']} multiple />);
    expect(screen.getByRole('button', { name: /Item A/ })).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByRole('button', { name: /Item B/ })).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('button', { name: /Item C/ })).toHaveAttribute('aria-expanded', 'true');
  });

  it('disabled item cannot be toggled', async () => {
    render(<Accordion items={disabledItems} />);
    const lockedBtn = screen.getByRole('button', { name: /Locked/ });
    expect(lockedBtn).toBeDisabled();
    await userEvent.click(lockedBtn);
    expect(lockedBtn).toHaveAttribute('aria-expanded', 'false');
  });

  it('header button has aria-controls pointing to panel', () => {
    render(<Accordion items={items} />);
    const btnA = screen.getByRole('button', { name: /Item A/ });
    const controlsId = btnA.getAttribute('aria-controls');
    expect(controlsId).toBeTruthy();
    expect(document.getElementById(controlsId!)).toBeInTheDocument();
  });

  it('panel has role="region" and aria-labelledby', () => {
    render(<Accordion items={items} />);
    const regions = screen.getAllByRole('region');
    expect(regions.length).toBe(3);
    regions.forEach((region) => {
      expect(region).toHaveAttribute('aria-labelledby');
    });
  });
});

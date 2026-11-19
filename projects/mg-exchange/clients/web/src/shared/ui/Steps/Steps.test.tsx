import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Steps } from './Steps';
import type { StepDef } from './Steps';

const steps: StepDef[] = [
  { key: 'a', label: 'Step One' },
  { key: 'b', label: 'Step Two' },
  { key: 'c', label: 'Step Three' },
];

const stepsWithDesc: StepDef[] = [
  { key: 'a', label: 'Identity',   description: 'Verify your identity' },
  { key: 'b', label: 'Address',    description: 'Confirm your address' },
  { key: 'c', label: 'Submit',     description: 'Final review' },
];

describe('Steps', () => {
  it('renders all step labels', () => {
    render(<Steps steps={steps} currentStep={1} />);
    expect(screen.getByText('Step One')).toBeInTheDocument();
    expect(screen.getByText('Step Two')).toBeInTheDocument();
    expect(screen.getByText('Step Three')).toBeInTheDocument();
  });

  it('active step has aria-current="step"', () => {
    render(<Steps steps={steps} currentStep={1} />);
    const listItems = screen.getAllByRole('listitem');
    expect(listItems[1]).toHaveAttribute('aria-current', 'step');
  });

  it('non-active steps do not have aria-current', () => {
    render(<Steps steps={steps} currentStep={1} />);
    const listItems = screen.getAllByRole('listitem');
    expect(listItems[0]).not.toHaveAttribute('aria-current');
    expect(listItems[2]).not.toHaveAttribute('aria-current');
  });

  it('completed steps show the checkmark', () => {
    render(<Steps steps={steps} currentStep={2} />);
    // Steps 0 and 1 are completed (index < 2)
    const checks = screen.getAllByText('✓');
    expect(checks).toHaveLength(2);
  });

  it('active step shows its number, not a checkmark', () => {
    render(<Steps steps={steps} currentStep={1} />);
    // Index 1 is active → shows "2"
    const numbers = screen.getAllByText('2');
    expect(numbers.length).toBeGreaterThanOrEqual(1);
    // No checkmark for active step when only first is completed
    const checks = screen.getAllByText('✓');
    expect(checks).toHaveLength(1); // only step 0 is completed
  });

  it('pending steps show their number', () => {
    render(<Steps steps={steps} currentStep={0} />);
    // Steps 1 and 2 are pending → show "2" and "3"
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('applies completed state to steps before currentStep', () => {
    const { container } = render(<Steps steps={steps} currentStep={2} />);
    const listItems = container.querySelectorAll('li');
    expect(listItems[0]).toHaveAttribute('data-state', 'completed');
    expect(listItems[1]).toHaveAttribute('data-state', 'completed');
  });

  it('applies active state to currentStep', () => {
    const { container } = render(<Steps steps={steps} currentStep={1} />);
    const listItems = container.querySelectorAll('li');
    expect(listItems[1]).toHaveAttribute('data-state', 'active');
  });

  it('applies pending state to steps after currentStep', () => {
    const { container } = render(<Steps steps={steps} currentStep={0} />);
    const listItems = container.querySelectorAll('li');
    expect(listItems[1]).toHaveAttribute('data-state', 'pending');
    expect(listItems[2]).toHaveAttribute('data-state', 'pending');
  });

  it('renders descriptions when provided', () => {
    render(<Steps steps={stepsWithDesc} currentStep={1} />);
    expect(screen.getByText('Verify your identity')).toBeInTheDocument();
    expect(screen.getByText('Confirm your address')).toBeInTheDocument();
    expect(screen.getByText('Final review')).toBeInTheDocument();
  });

  it('does not render description elements when not provided', () => {
    render(<Steps steps={steps} currentStep={0} />);
    const descriptions = document.querySelectorAll('[class*="description"]');
    expect(descriptions).toHaveLength(0);
  });

  it('applies horizontal direction by default', () => {
    const { container } = render(<Steps steps={steps} currentStep={0} />);
    const ol = container.querySelector('ol');
    expect(ol).toHaveAttribute('data-direction', 'horizontal');
  });

  it('applies vertical direction when direction="vertical"', () => {
    const { container } = render(<Steps steps={steps} currentStep={0} direction="vertical" />);
    const ol = container.querySelector('ol');
    expect(ol).toHaveAttribute('data-direction', 'vertical');
  });

  it('all steps show as completed when currentStep exceeds length', () => {
    render(<Steps steps={steps} currentStep={5} />);
    const checks = screen.getAllByText('✓');
    expect(checks).toHaveLength(3);
  });
});

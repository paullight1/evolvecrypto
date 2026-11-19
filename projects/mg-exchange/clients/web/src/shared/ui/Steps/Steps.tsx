import { cn } from '@shared/utils/format';
import styles from './Steps.module.css';

export interface StepDef {
  key: string;
  label: string;
  description?: string;
}

export interface StepsProps {
  steps: StepDef[];
  currentStep: number;
  direction?: 'horizontal' | 'vertical';
  className?: string;
}

type StepState = 'completed' | 'active' | 'pending';

function getStepState(index: number, currentStep: number): StepState {
  if (index < currentStep) return 'completed';
  if (index === currentStep) return 'active';
  return 'pending';
}

export function Steps({
  steps,
  currentStep,
  direction = 'horizontal',
  className,
}: StepsProps) {
  return (
    <ol
      className={cn(
        styles.steps,
        direction === 'vertical' ? styles.vertical : styles.horizontal,
        className,
      )}
      aria-label="progress"
      data-direction={direction}
    >
      {steps.map((step, i) => {
        const state = getStepState(i, currentStep);
        const isLast = i === steps.length - 1;

        return (
          <li
            key={step.key}
            className={cn(styles.step, styles[state])}
            data-state={state}
            aria-current={state === 'active' ? 'step' : undefined}
          >
            {/* Circle + connector wrapper */}
            <div className={styles.circleRow}>
              <div className={cn(styles.circle, styles[`circle-${state}`])}>
                {state === 'completed' ? (
                  <span className={styles.check} aria-hidden="true">✓</span>
                ) : (
                  <span className={styles.number}>{i + 1}</span>
                )}
              </div>

              {/* Connector line after circle, not rendered for the last step */}
              {!isLast && (
                <div
                  className={cn(
                    styles.connector,
                    state === 'completed' ? styles.connectorDone : styles.connectorPending,
                  )}
                  aria-hidden="true"
                />
              )}
            </div>

            {/* Labels */}
            <div className={styles.labelGroup}>
              <span className={cn(styles.label, styles[`label-${state}`])}>{step.label}</span>
              {step.description && (
                <span className={styles.description}>{step.description}</span>
              )}
            </div>
          </li>
        );
      })}
    </ol>
  );
}

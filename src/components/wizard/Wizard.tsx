import type { ReactNode } from 'react';
import './Wizard.css';

export type WizardStep = {
  id: string;
  title: string;
  render: () => ReactNode;
  canAdvance?: boolean;
};

export function Wizard({
  steps,
  currentIndex,
  onPrev,
  onNext,
  onCancel,
  onComplete,
  prevLabel,
  nextLabel,
  finishLabel,
  cancelLabel,
  pending,
  pendingLabel,
}: {
  steps: WizardStep[];
  currentIndex: number;
  onPrev: () => void;
  onNext: () => void;
  onCancel: () => void;
  onComplete: () => void;
  prevLabel: string;
  nextLabel: string;
  finishLabel: string;
  cancelLabel: string;
  pending?: boolean;
  pendingLabel?: string;
}) {
  if (steps.length === 0) {
    return null;
  }

  const safeIndex = Math.min(Math.max(currentIndex, 0), steps.length - 1);
  const current = steps[safeIndex];
  const isLast = safeIndex === steps.length - 1;
  const canAdvance = current.canAdvance ?? true;

  return (
    <div className="wizard">
      <div className="wizard-header">
        <ol className="wizard-step-bar" aria-label="Steps">
          {steps.map((step, index) => {
            const stateClass =
              index < safeIndex
                ? 'wizard-step-done'
                : index === safeIndex
                  ? 'wizard-step-active'
                  : 'wizard-step-pending';
            return (
              <li key={step.id} className={`wizard-step ${stateClass}`}>
                <span className="wizard-step-index">{index + 1}</span>
                <span className="wizard-step-title">{step.title}</span>
              </li>
            );
          })}
        </ol>
      </div>
      <div className="wizard-body">{current.render()}</div>
      <div className="wizard-footer">
        <div className="wizard-footer-left">
          <button type="button" className="wizard-btn" onClick={onCancel} disabled={pending}>
            {cancelLabel}
          </button>
        </div>
        <div className="wizard-footer-right">
          {safeIndex > 0 ? (
            <button type="button" className="wizard-btn" onClick={onPrev} disabled={pending}>
              {prevLabel}
            </button>
          ) : null}
          {isLast ? (
            <button
              type="button"
              className="wizard-btn wizard-btn-primary"
              onClick={onComplete}
              disabled={!canAdvance || pending}
            >
              {pending ? (pendingLabel ?? finishLabel) : finishLabel}
            </button>
          ) : (
            <button
              type="button"
              className="wizard-btn wizard-btn-primary"
              onClick={onNext}
              disabled={!canAdvance || pending}
            >
              {pending ? (pendingLabel ?? nextLabel) : nextLabel}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

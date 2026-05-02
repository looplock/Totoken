import './ConfirmDialog.css';
import { useEffect, useRef, useState, type ReactNode } from 'react';

export type ConfirmTone = 'default' | 'danger';

export function ConfirmDialog({
  open,
  title,
  description,
  descriptionExtra,
  confirmLabel,
  cancelLabel,
  tone = 'default',
  requireText,
  onConfirm,
  onCancel,
  pending,
}: {
  open: boolean;
  title: string;
  description?: string;
  /**
   * Extra block rendered between the description and the require-text input.
   * Used for dependency warnings or extra context where the caller wants
   * distinct visual treatment instead of jamming everything into the
   * description string.
   */
  descriptionExtra?: ReactNode;
  confirmLabel: string;
  cancelLabel: string;
  tone?: ConfirmTone;
  requireText?: string;
  onConfirm: () => void;
  onCancel: () => void;
  pending?: boolean;
}) {
  const [typed, setTyped] = useState('');
  const overlayMouseDownOnSelfRef = useRef(false);

  useEffect(() => {
    if (open) setTyped('');
  }, [open]);

  if (!open) return null;

  const blocked = requireText !== undefined && typed !== requireText;

  return (
    <div
      className="confirm-overlay"
      role="dialog"
      aria-modal="true"
      onMouseDown={(event) => {
        // Only treat the mousedown as a backdrop click when it lands on the
        // overlay itself. If the user starts a drag inside the modal (e.g.
        // selecting text in the require-text input) and releases over the
        // overlay, the resulting click would otherwise close the dialog.
        overlayMouseDownOnSelfRef.current = event.target === event.currentTarget;
      }}
      onClick={(event) => {
        const downOnSelf = overlayMouseDownOnSelfRef.current;
        overlayMouseDownOnSelfRef.current = false;
        if (downOnSelf && event.target === event.currentTarget && !pending) {
          onCancel();
        }
      }}
    >
      <div className="confirm-modal">
        <header className="confirm-header">
          <h3 className="confirm-title">{title}</h3>
        </header>
        {description ? <p className="confirm-description">{description}</p> : null}
        {descriptionExtra ? (
          <div className="confirm-description-extra">{descriptionExtra}</div>
        ) : null}
        {requireText !== undefined ? (
          <label className="confirm-typed">
            <span className="confirm-typed-label">{requireText}</span>
            <input
              type="text"
              className="confirm-typed-input"
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoFocus
              spellCheck={false}
            />
          </label>
        ) : null}
        <div className="confirm-actions">
          <button type="button" className="confirm-btn" onClick={onCancel} disabled={pending}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={
              tone === 'danger'
                ? 'confirm-btn confirm-btn-danger'
                : 'confirm-btn confirm-btn-primary'
            }
            onClick={onConfirm}
            disabled={blocked || pending}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

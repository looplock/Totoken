import { useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { IconButton } from '../icon-button/IconButton';
import './MaskedSecret.css';

type Mode = 'idle' | 'editing';

export function MaskedSecret({
  hasValue,
  placeholder,
  setLabel,
  modifyLabel,
  clearLabel,
  saveLabel,
  cancelLabel,
  showLabel,
  hideLabel,
  setLabelEmpty,
  onSubmit,
  onClear,
  disabled,
}: {
  hasValue: boolean;
  placeholder?: string;
  setLabel: string;
  setLabelEmpty: string;
  modifyLabel: string;
  clearLabel: string;
  saveLabel: string;
  cancelLabel: string;
  showLabel: string;
  hideLabel: string;
  onSubmit: (value: string) => Promise<void> | void;
  onClear?: () => Promise<void> | void;
  disabled?: boolean;
}) {
  const [mode, setMode] = useState<Mode>('idle');
  const [value, setValue] = useState('');
  const [reveal, setReveal] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');

  function startEditing() {
    setValue('');
    setReveal(false);
    setError('');
    setMode('editing');
  }

  function cancel() {
    setValue('');
    setError('');
    setMode('idle');
  }

  async function submit() {
    if (!value) {
      setError(setLabelEmpty);
      return;
    }
    setPending(true);
    setError('');
    try {
      await onSubmit(value);
      setValue('');
      setMode('idle');
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setPending(false);
    }
  }

  async function handleClear() {
    if (!onClear) return;
    setPending(true);
    setError('');
    try {
      await onClear();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setPending(false);
    }
  }

  if (mode === 'editing') {
    return (
      <div className="masked-secret masked-secret-editing">
        <div className="masked-secret-input-row">
          <input
            type={reveal ? 'text' : 'password'}
            className="masked-secret-input"
            placeholder={placeholder}
            value={value}
            disabled={pending}
            onChange={(event) => setValue(event.target.value)}
            autoFocus
          />
          <IconButton
            className="masked-secret-icon-button"
            onClick={() => setReveal((current) => !current)}
            label={reveal ? hideLabel : showLabel}
            disabled={pending}
          >
            {reveal ? <EyeOff size={16} /> : <Eye size={16} />}
          </IconButton>
        </div>
        <div className="masked-secret-actions">
          <button
            type="button"
            className="masked-secret-button masked-secret-button-primary"
            onClick={() => void submit()}
            disabled={pending}
          >
            {saveLabel}
          </button>
          <button
            type="button"
            className="masked-secret-button"
            onClick={cancel}
            disabled={pending}
          >
            {cancelLabel}
          </button>
        </div>
        {error ? <p className="masked-secret-error">{error}</p> : null}
      </div>
    );
  }

  return (
    <div className="masked-secret">
      <div className="masked-secret-status">
        <span
          className={
            hasValue
              ? 'masked-secret-status-dot masked-secret-status-dot-set'
              : 'masked-secret-status-dot masked-secret-status-dot-unset'
          }
          aria-hidden
        />
        <span className="masked-secret-status-label">{hasValue ? setLabel : setLabelEmpty}</span>
      </div>
      <div className="masked-secret-actions">
        <button
          type="button"
          className="masked-secret-button"
          onClick={startEditing}
          disabled={disabled || pending}
        >
          {hasValue ? modifyLabel : setLabel}
        </button>
        {hasValue && onClear ? (
          <button
            type="button"
            className="masked-secret-button masked-secret-button-danger"
            onClick={() => void handleClear()}
            disabled={disabled || pending}
          >
            {clearLabel}
          </button>
        ) : null}
      </div>
      {error ? <p className="masked-secret-error">{error}</p> : null}
    </div>
  );
}

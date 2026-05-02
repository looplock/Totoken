import './Switch.css';

type SwitchSize = 'sm' | 'md';

export function Switch({
  checked,
  disabled,
  label,
  size = 'md',
  onToggle,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  size?: SwitchSize;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className={['app-switch', `app-switch-${size}`, checked ? 'app-switch-on' : undefined]
        .filter(Boolean)
        .join(' ')}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={(event) => {
        event.stopPropagation();
        if (!disabled) {
          onToggle();
        }
      }}
    />
  );
}

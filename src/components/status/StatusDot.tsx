import './StatusDot.css';

export type StatusTone = 'success' | 'warning' | 'danger' | 'muted' | 'info';

export function StatusDot({
  tone,
  filled = true,
  label,
  className,
}: {
  tone: StatusTone;
  filled?: boolean;
  label?: string;
  className?: string;
}) {
  const classes = [
    'status-dot',
    `status-dot-${tone}`,
    filled ? 'status-dot-filled' : 'status-dot-hollow',
    className ?? '',
  ]
    .filter(Boolean)
    .join(' ');
  return <span className={classes} role="img" aria-label={label} data-tone={tone} />;
}

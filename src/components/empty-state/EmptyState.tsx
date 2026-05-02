import type { ElementType, ReactNode } from 'react';
import './EmptyState.css';

export type EmptyStateVariant = 'plain' | 'card' | 'fill';

export interface EmptyStateProps {
  children: ReactNode;
  icon?: ReactNode;
  variant?: EmptyStateVariant;
  compact?: boolean;
  as?: ElementType;
  className?: string;
}

export function EmptyState({
  children,
  icon,
  variant = 'plain',
  compact = false,
  as,
  className,
}: EmptyStateProps) {
  const Tag = (as ?? 'div') as ElementType;
  const classes = ['empty-state'];
  if (variant === 'card') classes.push('empty-state--card');
  if (variant === 'fill') classes.push('empty-state--fill');
  if (compact) classes.push('empty-state--compact');
  if (className) classes.push(className);

  return (
    <Tag className={classes.join(' ')}>
      {icon ? <span className="empty-state__icon">{icon}</span> : null}
      <div className="empty-state__body">{children}</div>
    </Tag>
  );
}

import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { Tooltip } from '../tooltip/Tooltip';

type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children' | 'title'> & {
  label: string;
  tooltip?: ReactNode;
  tooltipAlign?: 'start' | 'center' | 'end';
  showTooltip?: boolean;
  children: ReactNode;
};

export function IconButton({
  label,
  tooltip,
  tooltipAlign = 'center',
  showTooltip = true,
  type = 'button',
  children,
  ...buttonProps
}: IconButtonProps) {
  const button = (
    <button {...buttonProps} type={type} aria-label={label}>
      {children}
    </button>
  );

  if (!showTooltip) {
    return button;
  }

  return (
    <Tooltip content={tooltip ?? label} align={tooltipAlign}>
      {button}
    </Tooltip>
  );
}

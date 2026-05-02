import { Info } from 'lucide-react';
import { Tooltip } from './Tooltip';

type InfoTooltipProps = {
  content: string;
  label: string;
  align?: 'start' | 'center' | 'end';
  className?: string;
};

export function InfoTooltip({ content, label, align = 'start', className }: InfoTooltipProps) {
  return (
    <Tooltip content={content} align={align} className={className}>
      <button type="button" className="app-info-tooltip-button" aria-label={label}>
        <Info size={15} />
      </button>
    </Tooltip>
  );
}

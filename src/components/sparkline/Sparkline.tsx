import './Sparkline.css';

export type SparklineTone = 'accent' | 'success' | 'danger' | 'warning' | 'muted';

export interface SparklineProps {
  data: number[];
  width?: number;
  height?: number;
  tone?: SparklineTone;
  ariaLabel?: string;
  className?: string;
}

/**
 * Lightweight inline sparkline. Renders a smoothed line + soft fill area.
 * - Pure SVG, no dependencies.
 * - Inherits color via CSS variables; pass a `tone` to switch palette.
 * - Renders an inert dash when data has fewer than 2 points.
 */
export function Sparkline({
  data,
  width = 60,
  height = 18,
  tone = 'accent',
  ariaLabel,
  className,
}: SparklineProps) {
  const cleaned = data.filter((value) => Number.isFinite(value));
  if (cleaned.length < 2) {
    return (
      <span
        className={['sparkline', 'sparkline-empty', className ?? ''].filter(Boolean).join(' ')}
        style={{ width, height: 1 }}
        role="img"
        aria-label={ariaLabel}
      />
    );
  }

  const min = Math.min(...cleaned);
  const max = Math.max(...cleaned);
  const range = max - min || 1;
  const stepX = width / (cleaned.length - 1);
  const innerHeight = height - 2;

  const points = cleaned.map((value, index) => {
    const x = index * stepX;
    const normalized = (value - min) / range;
    const y = innerHeight - normalized * innerHeight + 1;
    return [x, y] as const;
  });

  const linePath = points
    .map(([x, y], index) => `${index === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`)
    .join(' ');

  const areaPath = `${linePath} L${points[points.length - 1][0].toFixed(2)},${height.toFixed(
    2,
  )} L${points[0][0].toFixed(2)},${height.toFixed(2)} Z`;

  const dataTone = tone === 'accent' ? undefined : tone;

  return (
    <span
      className={['sparkline', className ?? ''].filter(Boolean).join(' ')}
      data-tone={dataTone}
      role="img"
      aria-label={ariaLabel}
    >
      <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`}>
        <path className="sparkline-fill" d={areaPath} />
        <path className="sparkline-stroke" d={linePath} />
      </svg>
    </span>
  );
}

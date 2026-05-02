import { useId, useRef, useState } from 'react';
import type { CSSProperties, MouseEvent } from 'react';
import {
  buildAxisTicks,
  buildCompactModelUsageItems,
  createCompactCurrencyFormatter,
  formatCompactCurrencyValue,
} from './statisticsView';
import type { DistributionItem, DistributionMetric, ModelUsageItem } from './statisticsView';

export function DistributionChart({
  totalSessions,
  totalTokens,
  totalEstimatedCost,
  locale,
  tokenUnitLocale,
  metric,
  label,
  items,
  hoveredDistributionIndex,
  setHoveredDistributionIndex,
}: {
  totalSessions: number;
  totalTokens: number;
  totalEstimatedCost: number;
  locale: string;
  tokenUnitLocale: string;
  metric: DistributionMetric;
  label: string;
  items: DistributionItem[];
  hoveredDistributionIndex: number | null;
  setHoveredDistributionIndex: (index: number | null) => void;
}) {
  return (
    <div className="statistics-distribution">
      <DonutChart
        value={
          metric === 'sessions'
            ? totalSessions
            : metric === 'cost'
              ? totalEstimatedCost
              : totalTokens
        }
        label={label}
        metric={metric}
        items={items}
        locale={locale}
        tokenUnitLocale={tokenUnitLocale}
        hoveredIndex={hoveredDistributionIndex}
        onHoverChange={setHoveredDistributionIndex}
      />
    </div>
  );
}

export function ModelUsagePanel({
  items,
  locale,
  metric,
  compactFormatter,
  numberFormatter,
}: {
  items: ModelUsageItem[];
  locale: string;
  metric: DistributionMetric;
  compactFormatter: Intl.NumberFormat;
  numberFormatter: Intl.NumberFormat;
}) {
  const displayItems = buildCompactModelUsageItems(items, locale);
  const axisFormatter =
    metric === 'sessions'
      ? numberFormatter
      : metric === 'cost'
        ? createCompactCurrencyFormatter()
        : compactFormatter;
  const axisTicks = buildAxisTicks(items[0]?.value ?? 0, axisFormatter, 3);

  return (
    <div className="statistics-model-usage">
      <div>
        {displayItems.map((item) => (
          <div key={item.label} className="statistics-model-row">
            <span className="statistics-model-label" title={item.label}>
              {item.label}
            </span>
            <div className="statistics-model-bar">
              <span style={{ width: `${item.percent}%` }} />
            </div>
            <span className="statistics-model-share">{item.shareLabel}</span>
          </div>
        ))}
      </div>

      <div
        className="statistics-model-axis statistics-model-axis-compact"
        style={{ gridTemplateColumns: `repeat(${axisTicks.length}, minmax(0, 1fr))` }}
      >
        {axisTicks.map((tick) => (
          <span key={`${tick.value}-${tick.percent}`}>{tick.label}</span>
        ))}
      </div>
    </div>
  );
}
function DonutChart({
  value,
  label,
  metric,
  items,
  locale,
  tokenUnitLocale,
  hoveredIndex,
  onHoverChange,
}: {
  value: number;
  label: string;
  metric: DistributionMetric;
  items: DistributionItem[];
  locale: string;
  tokenUnitLocale: string;
  hoveredIndex: number | null;
  onHoverChange: (index: number | null) => void;
}) {
  const compactNumberFormatter = new Intl.NumberFormat(locale === 'zh' ? 'zh-CN' : 'en-US', {
    notation: 'compact',
    maximumFractionDigits: 1,
  });
  const tokenFormatter = new Intl.NumberFormat(tokenUnitLocale, {
    notation: 'compact',
    maximumFractionDigits: 1,
  });
  const panelRef = useRef<HTMLDivElement | null>(null);
  const donutRef = useRef<HTMLDivElement | null>(null);
  const gradientId = useId().replace(/:/g, '');
  const [tooltipPosition, setTooltipPosition] = useState<{
    left: number;
    top: number;
    side: 'left' | 'right';
    vertical: 'top' | 'bottom';
  } | null>(null);
  const total = items.reduce((sum, item) => sum + item.value, 0) || 1;
  const radius = 88;
  const strokeWidth = 34;
  const circumference = 2 * Math.PI * radius;
  let offset = 0;
  const activeIndex = hoveredIndex ?? (items.length > 0 ? 0 : null);
  const activeItem = activeIndex === null ? null : (items[activeIndex] ?? null);
  const showTooltip = hoveredIndex === null || activeItem === null || tooltipPosition === null;

  function updateTooltipPosition(event: MouseEvent<SVGCircleElement>, index: number) {
    const panel = panelRef.current;
    const donut = donutRef.current;
    if (!panel || !donut) {
      onHoverChange(index);
      return;
    }

    const panelRect = panel.getBoundingClientRect();
    const donutRect = donut.getBoundingClientRect();
    const left = event.clientX - panelRect.left;
    const top = event.clientY - panelRect.top;
    const centerX = donutRect.left - panelRect.left + donutRect.width / 2;
    const centerY = donutRect.top - panelRect.top + donutRect.height / 2;
    const deltaX = left - centerX;
    const deltaY = top - centerY;
    const distance = Math.hypot(deltaX, deltaY) || 1;
    const outwardOffset = 22;
    const sideThreshold = 22;
    const verticalThreshold = 18;
    const adjustedLeft = left + (deltaX / distance) * outwardOffset;
    const adjustedTop = top + (deltaY / distance) * outwardOffset;
    const side =
      deltaX < -sideThreshold
        ? 'left'
        : deltaX > sideThreshold
          ? 'right'
          : (tooltipPosition?.side ?? (deltaX < 0 ? 'left' : 'right'));
    const vertical =
      deltaY < -verticalThreshold
        ? 'top'
        : deltaY > verticalThreshold
          ? 'bottom'
          : (tooltipPosition?.vertical ?? (deltaY < 0 ? 'top' : 'bottom'));

    onHoverChange(index);
    setTooltipPosition({
      left: adjustedLeft,
      top: adjustedTop,
      side,
      vertical,
    });
  }

  function clearTooltip() {
    onHoverChange(null);
    setTooltipPosition(null);
  }

  function formatDistributionValue(rawValue: number) {
    if (metric === 'sessions') {
      return compactNumberFormatter.format(Math.round(rawValue));
    }

    if (metric === 'cost') {
      return formatCompactCurrencyValue(rawValue);
    }

    return tokenFormatter.format(rawValue);
  }

  return (
    <div className="statistics-donut-wrap">
      <div ref={panelRef} className="statistics-donut-panel">
        <div ref={donutRef} className="statistics-donut">
          <svg viewBox="0 0 240 240" aria-hidden="true">
            <defs>
              {items.map((item, index) => (
                <linearGradient
                  key={item.label}
                  id={`${gradientId}-${index}`}
                  x1="0%"
                  y1="0%"
                  x2="100%"
                  y2="100%"
                >
                  <stop offset="0%" stopColor={item.color} stopOpacity="0.88" />
                  <stop offset="100%" stopColor={item.color} stopOpacity="1" />
                </linearGradient>
              ))}
            </defs>
            <circle
              cx="120"
              cy="120"
              r={radius}
              fill="none"
              stroke="var(--color-border-soft)"
              strokeWidth={strokeWidth}
            />
            {items.map((item, index) => {
              const dashLength = (item.value / total) * circumference;
              const dashOffset = circumference * 0.25 - offset;
              offset += dashLength;
              return (
                <circle
                  key={item.label}
                  className={
                    hoveredIndex === index
                      ? 'statistics-donut-segment statistics-donut-segment-active'
                      : 'statistics-donut-segment'
                  }
                  cx="120"
                  cy="120"
                  r={radius}
                  fill="none"
                  stroke={`url(#${gradientId}-${index})`}
                  strokeWidth={strokeWidth}
                  strokeDasharray={`${dashLength} ${circumference - dashLength}`}
                  strokeDashoffset={dashOffset}
                  strokeLinecap="butt"
                  onMouseEnter={(event) => updateTooltipPosition(event, index)}
                  onMouseMove={(event) => updateTooltipPosition(event, index)}
                  onMouseLeave={clearTooltip}
                />
              );
            })}
          </svg>
          <div className="statistics-donut-hole">
            <strong>
              {formatDistributionValue(
                hoveredIndex === null ? value : (items[hoveredIndex]?.value ?? 0),
              )}
            </strong>
            <span>{hoveredIndex === null ? label : (items[hoveredIndex]?.label ?? label)}</span>
          </div>
        </div>
        <div
          className={
            !showTooltip
              ? `statistics-donut-tooltip statistics-donut-tooltip-visible ${
                  tooltipPosition?.side === 'left'
                    ? 'statistics-donut-tooltip-left'
                    : 'statistics-donut-tooltip-right'
                } ${
                  tooltipPosition?.vertical === 'bottom'
                    ? 'statistics-donut-tooltip-bottom'
                    : 'statistics-donut-tooltip-top'
                }`
              : 'statistics-donut-tooltip'
          }
          style={
            tooltipPosition
              ? ({
                  left: `${tooltipPosition.left}px`,
                  top: `${tooltipPosition.top}px`,
                } as CSSProperties)
              : undefined
          }
        >
          {showTooltip ? (
            <span>{locale === 'zh' ? '悬停查看应用占比' : 'Hover to inspect app share'}</span>
          ) : (
            <>
              <strong>{activeItem?.label ?? label}</strong>
              <div className="statistics-donut-tooltip-row">
                <span
                  className="statistics-donut-tooltip-dot"
                  style={{ background: activeItem?.color ?? '#ffbd5f' }}
                />
                <span className="statistics-donut-tooltip-name">{activeItem?.label ?? label}</span>
                <span className="statistics-donut-tooltip-value">
                  {formatDistributionValue(activeItem?.value ?? 0)}
                </span>
              </div>
              <span className="statistics-donut-tooltip-share">
                {activeItem?.shareLabel ?? '0.0%'}
              </span>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

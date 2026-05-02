import { useState } from 'react';
import type { CSSProperties, FocusEvent, MouseEvent } from 'react';
import {
  clamp,
  createEmptyActivityMatrix,
  formatActivityCellRange,
  formatActivityCellValue,
  formatActivityMetricTooltip,
} from './statisticsView';
import type { ActivityMetric } from './statisticsView';

export function HeatmapMatrix({
  locale,
  metric,
  matrix,
  maxValue,
}: {
  locale: string;
  metric: ActivityMetric;
  matrix: number[][];
  maxValue: number;
}) {
  const dayLabels =
    locale === 'zh'
      ? ['周一', '周二', '周三', '周四', '周五', '周六', '周日']
      : ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  const [hoveredCell, setHoveredCell] = useState<{
    dayLabel: string;
    hour: number;
    count: number;
    left: number;
    top: number;
    placement: 'top' | 'bottom';
  } | null>(null);
  const safeMatrix = matrix.length === 7 ? matrix : createEmptyActivityMatrix();

  const showTooltip = (
    event: MouseEvent<HTMLSpanElement> | FocusEvent<HTMLSpanElement>,
    dayLabel: string,
    hour: number,
    count: number,
  ) => {
    const cellRect = event.currentTarget.getBoundingClientRect();
    const tooltipHalfWidth = 108;
    const viewportPadding = 20;
    const centerX = cellRect.left + cellRect.width / 2;
    const clampedLeft = Math.min(
      Math.max(centerX, tooltipHalfWidth + viewportPadding),
      window.innerWidth - tooltipHalfWidth - viewportPadding,
    );
    const placement: 'top' | 'bottom' = cellRect.top >= 112 ? 'top' : 'bottom';

    setHoveredCell({
      dayLabel,
      hour,
      count,
      left: clampedLeft,
      top: placement === 'top' ? cellRect.top : cellRect.bottom,
      placement,
    });
  };

  return (
    <div className="statistics-activity-grid" onMouseLeave={() => setHoveredCell(null)}>
      <div className="statistics-activity-hours">
        <span />
        {Array.from({ length: 12 }, (_, index) => index * 2).map((hour) => (
          <span key={hour}>{String(hour).padStart(2, '0')}:00</span>
        ))}
      </div>

      <div className="statistics-activity-body">
        {safeMatrix.map((row, dayIndex) => (
          <div key={dayLabels[dayIndex]} className="statistics-activity-row">
            <span className="statistics-activity-day">{dayLabels[dayIndex]}</span>
            <div className="statistics-activity-cells">
              {row.map((cell, hour) => (
                <span
                  key={`${dayIndex}-${hour}`}
                  className="statistics-activity-cell"
                  style={{ opacity: maxValue > 0 ? clamp(cell / maxValue, 0.08, 1) : 0.08 }}
                  aria-label={formatActivityMetricTooltip(
                    locale,
                    dayLabels[dayIndex],
                    hour,
                    cell,
                    metric,
                  )}
                  tabIndex={0}
                  onMouseEnter={(event) => showTooltip(event, dayLabels[dayIndex], hour, cell)}
                  onFocus={(event) => showTooltip(event, dayLabels[dayIndex], hour, cell)}
                  onBlur={() => setHoveredCell(null)}
                />
              ))}
            </div>
          </div>
        ))}
      </div>

      {hoveredCell ? (
        <div
          className={
            hoveredCell.placement === 'bottom'
              ? 'statistics-activity-tooltip statistics-activity-tooltip-bottom'
              : 'statistics-activity-tooltip'
          }
          role="tooltip"
          style={
            {
              left: `${hoveredCell.left}px`,
              top: `${hoveredCell.top}px`,
            } as CSSProperties
          }
        >
          <div className="statistics-activity-tooltip-title">
            {formatActivityCellRange(hoveredCell.dayLabel, hoveredCell.hour)}
          </div>
          <div className="statistics-activity-tooltip-value">
            {formatActivityCellValue(locale, hoveredCell.count, metric)}
          </div>
        </div>
      ) : null}

      <div className="statistics-activity-legend">
        <span>{locale === 'zh' ? '少' : 'Less'}</span>
        <div className="statistics-activity-legend-bar">
          <span />
          <span />
          <span />
          <span />
          <span />
        </div>
        <span>{locale === 'zh' ? '多' : 'More'}</span>
      </div>
    </div>
  );
}

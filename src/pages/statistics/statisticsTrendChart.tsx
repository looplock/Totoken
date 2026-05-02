import { useId, useState } from 'react';
import type { CSSProperties } from 'react';
import type { StatisticsGranularity } from './statisticsTypes';
import {
  averagePointY,
  buildChartPoints,
  buildSmoothAreaPath,
  buildSmoothChartPath,
  buildTrendAxisScale,
  formatAxisNumber,
  formatChartTooltipCurrencyValue,
  formatChartTooltipTokenValue,
  formatCurrencyAxisNumber,
  getChartX,
  getChartY,
  shouldRenderTrendLabel,
} from './statisticsView';
import type { TrendSeriesKey } from './statisticsView';

export function TrendChart({
  locale,
  granularity,
  labels,
  cacheRead,
  cacheWrite,
  input,
  output,
  total,
  cost,
  visibleSeries,
  tokenUnitLocale,
  t,
}: {
  locale: string;
  granularity: StatisticsGranularity;
  labels: string[];
  cacheRead: number[];
  cacheWrite: number[];
  input: number[];
  output: number[];
  total: number[];
  cost: number[];
  visibleSeries: Record<TrendSeriesKey, boolean>;
  tokenUnitLocale: string;
  t: (key: string) => string;
}) {
  const pointCount = Math.max(
    labels.length,
    cacheRead.length,
    cacheWrite.length,
    input.length,
    output.length,
    total.length,
    cost.length,
    1,
  );
  const fallbackLabel = locale === 'zh' ? '暂无数据' : 'No data';
  const safeLabels = Array.from({ length: pointCount }, (_, index) => labels[index] ?? '');
  if (labels.length === 0) {
    safeLabels[0] = fallbackLabel;
  }

  const safeCacheRead = Array.from({ length: pointCount }, (_, index) => cacheRead[index] ?? 0);
  const safeCacheWrite = Array.from({ length: pointCount }, (_, index) => cacheWrite[index] ?? 0);
  const safeInput = Array.from({ length: pointCount }, (_, index) => input[index] ?? 0);
  const safeOutput = Array.from({ length: pointCount }, (_, index) => output[index] ?? 0);
  const safeTotal = Array.from({ length: pointCount }, (_, index) => total[index] ?? 0);
  const safeCost = Array.from({ length: pointCount }, (_, index) => cost[index] ?? 0);
  const width = 880;
  const height = 218;
  const padding = { top: 20, right: 42, bottom: 24, left: 38 };
  const yTicks = 5;
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const tokenScaleValues = [
    ...(visibleSeries['cache-read'] ? safeCacheRead : []),
    ...(visibleSeries['cache-write'] ? safeCacheWrite : []),
    ...(visibleSeries.input ? safeInput : []),
    ...(visibleSeries.output ? safeOutput : []),
    ...(visibleSeries.total ? safeTotal : []),
  ];
  const costScaleValues = visibleSeries.cost ? safeCost : [];
  const hasVisibleSeries = tokenScaleValues.length > 0 || costScaleValues.length > 0;
  const { maxValue, tickValues } = buildTrendAxisScale(tokenScaleValues, yTicks);
  const { maxValue: maxCostValue, tickValues: costTickValues } = buildTrendAxisScale(
    costScaleValues,
    yTicks,
  );
  const cacheReadPoints = buildChartPoints(safeCacheRead, width, height, padding, maxValue);
  const cacheWritePoints = buildChartPoints(safeCacheWrite, width, height, padding, maxValue);
  const inputPoints = buildChartPoints(safeInput, width, height, padding, maxValue);
  const outputPoints = buildChartPoints(safeOutput, width, height, padding, maxValue);
  const totalPoints = buildChartPoints(safeTotal, width, height, padding, maxValue);
  const costPoints = buildChartPoints(safeCost, width, height, padding, maxCostValue);
  const cacheReadPath = buildSmoothChartPath(cacheReadPoints);
  const cacheWritePath = buildSmoothChartPath(cacheWritePoints);
  const inputPath = buildSmoothChartPath(inputPoints);
  const outputPath = buildSmoothChartPath(outputPoints);
  const totalPath = buildSmoothChartPath(totalPoints);
  const costPath = buildSmoothChartPath(costPoints);
  const totalAreaPath = buildSmoothAreaPath(totalPoints, height, padding);
  const activeIndex = hoveredIndex ?? pointCount - 1;
  const activeX = getChartX(activeIndex, pointCount, width, padding);
  const showHoverState = hoveredIndex !== null && hasVisibleSeries;
  const gradientId = useId().replace(/:/g, '');
  const tooltipStyle = {
    left: `${(activeX / width) * 100}%`,
  } satisfies CSSProperties;
  const tooltipClassName = [
    'statistics-trend-tooltip',
    hoveredIndex === null ? undefined : 'statistics-trend-tooltip-visible',
    activeX > width * 0.7 ? 'statistics-trend-tooltip-right' : 'statistics-trend-tooltip-left',
  ]
    .filter(Boolean)
    .join(' ');
  const tooltipRows = [
    {
      key: 'cost',
      label: t('statistics.chart.cost'),
      value: safeCost[activeIndex],
      className: 'statistics-trend-tooltip-swatch statistics-trend-tooltip-swatch-cost',
      formatter: formatChartTooltipCurrencyValue,
    },
    {
      key: 'input',
      label: t('statistics.chart.input'),
      value: safeInput[activeIndex],
      className: 'statistics-trend-tooltip-swatch statistics-trend-tooltip-swatch-input',
      formatter: formatChartTooltipTokenValue,
    },
    {
      key: 'output',
      label: t('statistics.chart.output'),
      value: safeOutput[activeIndex],
      className: 'statistics-trend-tooltip-swatch statistics-trend-tooltip-swatch-output',
      formatter: formatChartTooltipTokenValue,
    },
    {
      key: 'cache-read',
      label: t('messagePage.chip.cacheRead'),
      value: safeCacheRead[activeIndex],
      className: 'statistics-trend-tooltip-swatch statistics-trend-tooltip-swatch-cache-read',
      formatter: formatChartTooltipTokenValue,
    },
    {
      key: 'cache-write',
      label: t('messagePage.chip.cacheWrite'),
      value: safeCacheWrite[activeIndex],
      className: 'statistics-trend-tooltip-swatch statistics-trend-tooltip-swatch-cache-write',
      formatter: formatChartTooltipTokenValue,
    },
    {
      key: 'total',
      label: t('statistics.chart.total'),
      value: safeTotal[activeIndex],
      className: 'statistics-trend-tooltip-swatch statistics-trend-tooltip-swatch-total',
      formatter: formatChartTooltipTokenValue,
    },
  ].filter((row) => visibleSeries[row.key as TrendSeriesKey]);
  const series = [
    {
      key: 'cost',
      path: costPath,
      points: costPoints,
      color: '#18a957',
      strokeWidth: 1.45,
      areaSize: safeCost.reduce((sum, value) => sum + value, 0),
      averageY: averagePointY(costPoints),
      areaPath: buildSmoothAreaPath(costPoints, height, padding),
      areaGradientId: `${gradientId}-area-cost`,
      areaStops: [
        { offset: '0%', opacity: 0.15 },
        { offset: '42%', opacity: 0.075 },
        { offset: '100%', opacity: 0 },
      ],
      haloClassName: 'statistics-trend-point-halo statistics-trend-point-halo-cost',
      activeClassName: 'statistics-trend-point-active statistics-trend-point-active-cost',
      coreClassName: 'statistics-trend-point-core statistics-trend-point-core-cost',
    },
    {
      key: 'input',
      path: inputPath,
      points: inputPoints,
      color: '#2f6df6',
      strokeWidth: 1.35,
      areaSize: safeInput.reduce((sum, value) => sum + value, 0),
      averageY: averagePointY(inputPoints),
      areaPath: buildSmoothAreaPath(inputPoints, height, padding),
      areaGradientId: `${gradientId}-area-input`,
      areaStops: [
        { offset: '0%', opacity: 0.16 },
        { offset: '45%', opacity: 0.08 },
        { offset: '100%', opacity: 0 },
      ],
      haloClassName: 'statistics-trend-point-halo statistics-trend-point-halo-input',
      activeClassName: 'statistics-trend-point-active statistics-trend-point-active-input',
      coreClassName: 'statistics-trend-point-core statistics-trend-point-core-input',
    },
    {
      key: 'output',
      path: outputPath,
      points: outputPoints,
      color: '#8658f6',
      strokeWidth: 1.35,
      areaSize: safeOutput.reduce((sum, value) => sum + value, 0),
      averageY: averagePointY(outputPoints),
      areaPath: buildSmoothAreaPath(outputPoints, height, padding),
      areaGradientId: `${gradientId}-area-output`,
      areaStops: [
        { offset: '0%', opacity: 0.15 },
        { offset: '45%', opacity: 0.075 },
        { offset: '100%', opacity: 0 },
      ],
      haloClassName: 'statistics-trend-point-halo statistics-trend-point-halo-output',
      activeClassName: 'statistics-trend-point-active statistics-trend-point-active-output',
      coreClassName: 'statistics-trend-point-core statistics-trend-point-core-output',
    },
    {
      key: 'cache-write',
      path: cacheWritePath,
      points: cacheWritePoints,
      color: '#ea4aaa',
      strokeWidth: 1.15,
      areaSize: safeCacheWrite.reduce((sum, value) => sum + value, 0),
      averageY: averagePointY(cacheWritePoints),
      areaPath: buildSmoothAreaPath(cacheWritePoints, height, padding),
      areaGradientId: `${gradientId}-area-cache-write`,
      areaStops: [
        { offset: '0%', opacity: 0.14 },
        { offset: '45%', opacity: 0.07 },
        { offset: '100%', opacity: 0 },
      ],
      haloClassName: 'statistics-trend-point-halo statistics-trend-point-halo-cache-write',
      activeClassName: 'statistics-trend-point-active statistics-trend-point-active-cache-write',
      coreClassName: 'statistics-trend-point-core statistics-trend-point-core-cache-write',
    },
    {
      key: 'cache-read',
      path: cacheReadPath,
      points: cacheReadPoints,
      color: '#21b9cf',
      strokeWidth: 1.15,
      areaSize: safeCacheRead.reduce((sum, value) => sum + value, 0),
      averageY: averagePointY(cacheReadPoints),
      areaPath: buildSmoothAreaPath(cacheReadPoints, height, padding),
      areaGradientId: `${gradientId}-area-cache-read`,
      areaStops: [
        { offset: '0%', opacity: 0.14 },
        { offset: '45%', opacity: 0.07 },
        { offset: '100%', opacity: 0 },
      ],
      haloClassName: 'statistics-trend-point-halo statistics-trend-point-halo-cache-read',
      activeClassName: 'statistics-trend-point-active statistics-trend-point-active-cache-read',
      coreClassName: 'statistics-trend-point-core statistics-trend-point-core-cache-read',
    },
    {
      key: 'total',
      path: totalPath,
      points: totalPoints,
      color: '#ff8a00',
      strokeWidth: 1.4,
      areaSize: safeTotal.reduce((sum, value) => sum + value, 0),
      averageY: averagePointY(totalPoints),
      areaPath: totalAreaPath,
      areaGradientId: `${gradientId}-area-total`,
      areaStops: [
        { offset: '0%', opacity: 0.18 },
        { offset: '40%', opacity: 0.1 },
        { offset: '100%', opacity: 0 },
      ],
      haloClassName: 'statistics-trend-point-halo statistics-trend-point-halo-total',
      activeClassName: 'statistics-trend-point-active statistics-trend-point-active-total',
      coreClassName: 'statistics-trend-point-core statistics-trend-point-core-total',
    },
  ] as const;
  const visibleSeriesItems = series.filter((item) => visibleSeries[item.key]);
  const visibleAreaSeriesItems = [...visibleSeriesItems]
    .sort((left, right) => left.averageY - right.averageY || right.areaSize - left.areaSize)
    .map((item, index, items) => ({
      ...item,
      areaMaskId: index < items.length - 1 ? `${gradientId}-mask-${item.key}` : undefined,
      coveringAreas: items.slice(index + 1),
    }));

  return (
    <div className="statistics-trend-chart" onMouseLeave={() => setHoveredIndex(null)}>
      <div className="statistics-trend-plot">
        <svg viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
          <defs>
            {visibleAreaSeriesItems.map((item) => (
              <linearGradient
                key={item.areaGradientId}
                id={item.areaGradientId}
                x1="0%"
                y1="0%"
                x2="0%"
                y2="100%"
              >
                {item.areaStops.map((stop) => (
                  <stop
                    key={`${item.areaGradientId}-${stop.offset}`}
                    offset={stop.offset}
                    stopColor={item.color}
                    stopOpacity={stop.opacity}
                  />
                ))}
              </linearGradient>
            ))}
            {visibleAreaSeriesItems.map((item) =>
              item.areaMaskId ? (
                <mask key={item.areaMaskId} id={item.areaMaskId} maskUnits="userSpaceOnUse">
                  <rect x="0" y="0" width={width} height={height} fill="#ffffff" />
                  {item.coveringAreas.map((coveringItem) => (
                    <path
                      key={`${item.areaMaskId}-${coveringItem.key}`}
                      d={coveringItem.areaPath}
                      fill="#000000"
                    />
                  ))}
                </mask>
              ) : null,
            )}
          </defs>
          <line
            x1={padding.left}
            x2={padding.left}
            y1={padding.top}
            y2={height - padding.bottom}
            className="statistics-trend-frame-line"
          />
          <line
            x1={width - padding.right}
            x2={width - padding.right}
            y1={padding.top}
            y2={height - padding.bottom}
            className="statistics-trend-frame-line statistics-trend-frame-line-right"
          />
          {tickValues.map((tickValue, index) => {
            const y = getChartY(tickValue, height, padding, maxValue);
            return (
              <g key={tickValue}>
                <line
                  x1={padding.left}
                  x2={width - padding.right}
                  y1={y}
                  y2={y}
                  className={
                    index === yTicks - 1
                      ? 'statistics-trend-grid-line statistics-trend-grid-line-baseline'
                      : 'statistics-trend-grid-line'
                  }
                />
                {tokenScaleValues.length > 0 ? (
                  <text
                    x={padding.left - 10}
                    y={y + 4}
                    textAnchor="end"
                    className="statistics-trend-axis-label statistics-trend-axis-label-y"
                  >
                    {formatAxisNumber(tickValue, tokenUnitLocale)}
                  </text>
                ) : null}
              </g>
            );
          })}
          {visibleSeries.cost
            ? costTickValues.map((tickValue) => {
                const y = getChartY(tickValue, height, padding, maxCostValue);
                return (
                  <text
                    key={`cost-${tickValue}`}
                    x={width - padding.right + 8}
                    y={y + 4}
                    textAnchor="start"
                    className="statistics-trend-axis-label statistics-trend-axis-label-y statistics-trend-axis-label-y-right"
                  >
                    {formatCurrencyAxisNumber(tickValue, locale)}
                  </text>
                );
              })
            : null}

          {visibleAreaSeriesItems.map((item) => (
            <path
              key={`${item.key}-area`}
              d={item.areaPath}
              fill={`url(#${item.areaGradientId})`}
              mask={item.areaMaskId ? `url(#${item.areaMaskId})` : undefined}
              className="statistics-trend-area"
            />
          ))}

          {visibleSeriesItems.map((item) => (
            <path
              key={item.key}
              d={item.path}
              fill="none"
              stroke={item.color}
              strokeWidth={item.strokeWidth}
              strokeLinecap="round"
              strokeLinejoin="round"
              className="statistics-line-path"
            />
          ))}

          {showHoverState ? (
            <>
              <line
                x1={activeX}
                x2={activeX}
                y1={padding.top}
                y2={height - padding.bottom}
                className="statistics-trend-focus-line"
              />
            </>
          ) : null}

          {showHoverState
            ? visibleSeriesItems.map((item) => {
                const point = item.points[activeIndex];
                if (!point) {
                  return null;
                }

                return (
                  <g key={`${item.key}-${activeIndex}`}>
                    <circle cx={point.x} cy={point.y} r="4.2" className={item.haloClassName} />
                    <circle cx={point.x} cy={point.y} r="2.65" className={item.activeClassName} />
                    <circle cx={point.x} cy={point.y} r="1.35" className={item.coreClassName} />
                  </g>
                );
              })
            : null}

          {safeLabels.map((label, index) => {
            if (!shouldRenderTrendLabel(index, safeLabels.length, granularity)) {
              return null;
            }

            const x = getChartX(index, safeLabels.length, width, padding);
            return (
              <g key={`${label}-${index}`}>
                <line
                  x1={x}
                  x2={x}
                  y1={height - padding.bottom + 2}
                  y2={height - padding.bottom + 6}
                  className="statistics-trend-axis-tick"
                />
                <text
                  x={x}
                  y={height - 10}
                  textAnchor="middle"
                  className={
                    showHoverState && hoveredIndex === index
                      ? 'statistics-trend-axis-label statistics-trend-axis-label-x statistics-trend-axis-label-active'
                      : 'statistics-trend-axis-label statistics-trend-axis-label-x'
                  }
                >
                  {label}
                </text>
              </g>
            );
          })}

          {Array.from({ length: pointCount }, (_, index) => index).map((index) => {
            const centerX = getChartX(index, pointCount, width, padding);
            const previousX =
              index === 0
                ? padding.left
                : (centerX + getChartX(index - 1, pointCount, width, padding)) / 2;
            const nextX =
              index === pointCount - 1
                ? width - padding.right
                : (centerX + getChartX(index + 1, pointCount, width, padding)) / 2;

            return (
              <rect
                key={`hover-${index}`}
                x={previousX}
                y={padding.top}
                width={Math.max(1, nextX - previousX)}
                height={height - padding.top - padding.bottom}
                fill="transparent"
                className="statistics-trend-hover-band"
                onMouseEnter={() => setHoveredIndex(index)}
              />
            );
          })}
        </svg>
        {tooltipRows.length > 0 ? (
          <div className={tooltipClassName} style={tooltipStyle}>
            <strong>{safeLabels[activeIndex] || fallbackLabel}</strong>
            <div className="statistics-trend-tooltip-grid">
              {tooltipRows.map((row) => (
                <div
                  key={row.key}
                  className={
                    row.key === 'total'
                      ? 'statistics-trend-tooltip-row statistics-trend-tooltip-row-total'
                      : 'statistics-trend-tooltip-row'
                  }
                >
                  <span className={row.className} />
                  <span className="statistics-trend-tooltip-label">{row.label}</span>
                  <span className="statistics-trend-tooltip-value">
                    {row.formatter(row.value, row.key === 'cost' ? locale : tokenUnitLocale)}
                  </span>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

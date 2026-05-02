import { Calendar, Search } from 'lucide-react';
import type { SourceApp } from '../../lib/sourceApps';
import type {
  StatisticsGranularity,
  StatisticsOverview,
  StatisticsPeriodFilter,
} from './statisticsTypes';
import { SelectField } from './statisticsComponents';
import {
  formatRangeLabel,
  getDefaultGranularityForPeriod,
  granularityOptions,
  periodOptions,
} from './statisticsView';

type StatisticsFiltersCopy = {
  customStart: string;
  customEnd: string;
};

type StatisticsFiltersProps = {
  locale: string;
  t: (key: string) => string;
  copy: StatisticsFiltersCopy;
  search: string;
  selectedApp: 'all' | SourceApp;
  period: StatisticsPeriodFilter;
  granularity: StatisticsGranularity;
  modelFilter: string;
  sourceFilter: 'all' | SourceApp;
  customStartDate: string;
  customEndDate: string;
  appTabs: Array<'all' | SourceApp>;
  enabledSourceApps: SourceApp[];
  availableModels: string[];
  range: StatisticsOverview['range'] | undefined;
  onSearchChange: (value: string) => void;
  onSelectedAppChange: (value: 'all' | SourceApp) => void;
  onPeriodChange: (value: StatisticsPeriodFilter) => void;
  onGranularityChange: (value: StatisticsGranularity) => void;
  onModelFilterChange: (value: string) => void;
  onSourceFilterChange: (value: 'all' | SourceApp) => void;
  onCustomStartDateChange: (value: string) => void;
  onCustomEndDateChange: (value: string) => void;
  onResetPage: () => void;
};

export function StatisticsFilters({
  locale,
  t,
  copy,
  search,
  selectedApp,
  period,
  granularity,
  modelFilter,
  sourceFilter,
  customStartDate,
  customEndDate,
  appTabs,
  enabledSourceApps,
  availableModels,
  range,
  onSearchChange,
  onSelectedAppChange,
  onPeriodChange,
  onGranularityChange,
  onModelFilterChange,
  onSourceFilterChange,
  onCustomStartDateChange,
  onCustomEndDateChange,
  onResetPage,
}: StatisticsFiltersProps) {
  return (
    <>
      <section className="statistics-toolbar">
        <label className="statistics-search" aria-label={t('statistics.search.placeholder')}>
          <Search size={17} />
          <input
            type="search"
            value={search}
            placeholder={t('statistics.search.placeholder')}
            onChange={(event) => {
              onSearchChange(event.target.value);
              onResetPage();
            }}
          />
        </label>

        <div className="statistics-toolbar-actions">
          <div className="statistics-segmented">
            {periodOptions.map((option) => (
              <button
                key={option}
                type="button"
                className={period === option ? 'statistics-segmented-active' : undefined}
                onClick={() => {
                  onPeriodChange(option);
                  onGranularityChange(getDefaultGranularityForPeriod(option));
                  onResetPage();
                }}
              >
                {option === 'custom' ? t('statistics.period.custom') : option.toUpperCase()}
              </button>
            ))}
          </div>

          {period === 'custom' ? (
            <div className="statistics-date-range">
              <label className="statistics-date-input">
                <Calendar size={16} />
                <input
                  type="date"
                  aria-label={copy.customStart}
                  value={customStartDate}
                  onChange={(event) => onCustomStartDateChange(event.target.value)}
                />
              </label>
              <span className="statistics-date-separator">~</span>
              <label className="statistics-date-input">
                <Calendar size={16} />
                <input
                  type="date"
                  aria-label={copy.customEnd}
                  value={customEndDate}
                  min={customStartDate}
                  onChange={(event) => onCustomEndDateChange(event.target.value)}
                />
              </label>
            </div>
          ) : (
            <div className="statistics-date-range">
              <label className="statistics-date-input">
                <Calendar size={16} />
                <input
                  type="date"
                  aria-label={copy.customEnd}
                  value={customEndDate}
                  onChange={(event) => onCustomEndDateChange(event.target.value)}
                />
              </label>
              <button type="button" className="statistics-btn" disabled>
                <span>{formatRangeLabel(range, locale)}</span>
              </button>
            </div>
          )}

          <div className="statistics-segmented">
            {granularityOptions.map((option) => (
              <button
                key={option}
                type="button"
                className={granularity === option ? 'statistics-segmented-active' : undefined}
                onClick={() => onGranularityChange(option)}
              >
                {t(`statistics.granularity.${option}`)}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className="statistics-filters">
        <div className="statistics-tabs">
          {appTabs.map((app) => (
            <button
              key={app}
              type="button"
              className={
                selectedApp === app ? 'statistics-tab statistics-tab-active' : 'statistics-tab'
              }
              onClick={() => {
                onSelectedAppChange(app);
                onResetPage();
              }}
            >
              {app === 'all' ? t('session.tabs.all') : t(`session.source.${app}`)}
            </button>
          ))}
        </div>

        <div className="statistics-filter-selects">
          <SelectField
            value={modelFilter}
            onChange={(value) => {
              onModelFilterChange(value);
              onResetPage();
            }}
            options={[
              { value: 'all', label: t('statistics.filter.allModels') },
              ...availableModels.map((model) => ({ value: model, label: model })),
            ]}
          />
          <SelectField
            value={sourceFilter}
            onChange={(value) => {
              onSourceFilterChange(value as 'all' | SourceApp);
              onResetPage();
            }}
            options={[
              { value: 'all', label: t('statistics.filter.allSources') },
              ...enabledSourceApps.map((app) => ({
                value: app,
                label: t(`session.source.${app}`),
              })),
            ]}
          />
        </div>
      </section>
    </>
  );
}

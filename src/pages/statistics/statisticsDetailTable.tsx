import { ChevronLeft, ChevronRight } from 'lucide-react';
import { EmptyState } from '../../components/empty-state/EmptyState';
import { InfoTooltip } from '../../components/tooltip/InfoTooltip';
import type { StatisticsDetailRow } from './statisticsTypes';
import { SelectField, StatisticsRow } from './statisticsComponents';
import type { PageToken } from './statisticsView';

type StatisticsDetailTableProps = {
  loading: boolean;
  loadingLabel: string;
  sectionInfoLabel: string;
  rows: StatisticsDetailRow[];
  rowsPerPage: number;
  currentPage: number;
  totalPages: number;
  pageTokens: PageToken[];
  locale: string;
  numberFormatter: Intl.NumberFormat;
  t: (key: string) => string;
  onRowsPerPageChange: (value: number) => void;
  onPageChange: (value: number) => void;
};

export function StatisticsDetailTable({
  loading,
  loadingLabel,
  sectionInfoLabel,
  rows,
  rowsPerPage,
  currentPage,
  totalPages,
  pageTokens,
  locale,
  numberFormatter,
  t,
  onRowsPerPageChange,
  onPageChange,
}: StatisticsDetailTableProps) {
  return (
    <article className="statistics-card statistics-detail-card">
      <header className="statistics-card-header statistics-detail-header">
        <div className="statistics-card-title">
          <h2>{t('statistics.section.detail')}</h2>
          <InfoTooltip label={sectionInfoLabel} content={t('statistics.info.detail')} />
        </div>
      </header>

      {loading ? (
        <EmptyState>{loadingLabel}</EmptyState>
      ) : rows.length > 0 ? (
        <>
          <div className="statistics-table-wrap">
            <table className="statistics-table">
              <thead>
                <tr>
                  <th>{t('statistics.detail.app')}</th>
                  <th>{t('session.header.model')}</th>
                  <th>{t('statistics.metric.totalSessions')}</th>
                  <th>{t('session.header.input')}</th>
                  <th>{t('session.header.output')}</th>
                  <th>{t('session.header.total')}</th>
                  <th>{t('statistics.activity.cost')}</th>
                  <th>{t('statistics.metric.avgTokensPerSession')}</th>
                  <th>{t('statistics.detail.lastActive')}</th>
                  <th>{t('statistics.detail.trend')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <StatisticsRow
                    key={row.id}
                    locale={locale}
                    row={row}
                    numberFormatter={numberFormatter}
                    t={t}
                  />
                ))}
              </tbody>
            </table>
          </div>

          <footer className="statistics-pagination">
            <div className="statistics-page-size">
              <span>{t('session.rows')}:</span>
              <SelectField
                compact
                value={String(rowsPerPage)}
                onChange={(value) => onRowsPerPageChange(Number(value))}
                options={[
                  { value: '10', label: '10' },
                  { value: '25', label: '25' },
                  { value: '50', label: '50' },
                ]}
              />
            </div>

            <div className="statistics-page-controls">
              <button
                type="button"
                className="statistics-page-btn"
                disabled={currentPage === 1}
                onClick={() => onPageChange(Math.max(1, currentPage - 1))}
              >
                <ChevronLeft size={16} />
                <span>{t('session.previous')}</span>
              </button>

              {pageTokens.map((token) =>
                typeof token === 'number' ? (
                  <button
                    key={token}
                    type="button"
                    className="statistics-page-btn"
                    aria-current={currentPage === token ? 'page' : undefined}
                    onClick={() => onPageChange(token)}
                  >
                    {token}
                  </button>
                ) : (
                  <span key={token} className="statistics-page-ellipsis">
                    ...
                  </span>
                ),
              )}

              <button
                type="button"
                className="statistics-page-btn"
                disabled={currentPage === totalPages}
                onClick={() => onPageChange(Math.min(totalPages, currentPage + 1))}
              >
                <span>{t('session.next')}</span>
                <ChevronRight size={16} />
              </button>
            </div>
          </footer>
        </>
      ) : (
        <EmptyState>{t('statistics.empty')}</EmptyState>
      )}
    </article>
  );
}

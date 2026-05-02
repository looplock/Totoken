import './DiffViewer.css';

export type ManagedKeyChange = {
  label: string;
  action: 'unchanged' | 'added' | 'removed' | 'changed';
  before: string | null;
  after: string | null;
};

export function DiffViewer({
  changes,
  emptyLabel,
  hideUnchanged = true,
}: {
  changes: ManagedKeyChange[];
  emptyLabel: string;
  hideUnchanged?: boolean;
}) {
  const visible = hideUnchanged
    ? changes.filter((change) => change.action !== 'unchanged')
    : changes;

  if (visible.length === 0) {
    return <p className="diff-viewer-empty">{emptyLabel}</p>;
  }

  return (
    <div className="diff-viewer">
      {visible.map((change, index) => (
        <div
          key={`${change.label}-${index}`}
          className={`diff-row diff-row-${change.action}`}
          data-action={change.action}
        >
          <div className="diff-row-label">
            <span className={`diff-action-tag diff-action-tag-${change.action}`}>
              {actionGlyph(change.action)}
            </span>
            <span className="diff-row-key">{change.label}</span>
          </div>
          <div className="diff-row-values">
            {change.before !== null ? (
              <code className="diff-value diff-value-before">{change.before}</code>
            ) : (
              <code className="diff-value diff-value-empty">—</code>
            )}
            <span className="diff-arrow">→</span>
            {change.after !== null ? (
              <code className="diff-value diff-value-after">{change.after}</code>
            ) : (
              <code className="diff-value diff-value-empty">—</code>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

function actionGlyph(action: ManagedKeyChange['action']): string {
  switch (action) {
    case 'added':
      return '+';
    case 'removed':
      return '−';
    case 'changed':
      return '~';
    default:
      return '·';
  }
}

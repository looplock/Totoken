import { useMemo } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { IconButton } from '../icon-button/IconButton';
import './KeyValueEditor.css';

export type KeyValuePair = {
  id: string;
  key: string;
  value: string;
};

export function KeyValueEditor({
  pairs,
  onChange,
  keyPlaceholder,
  valuePlaceholder,
  addLabel,
  emptyLabel,
  removeLabel = 'Remove',
}: {
  pairs: KeyValuePair[];
  onChange: (next: KeyValuePair[]) => void;
  keyPlaceholder: string;
  valuePlaceholder: string;
  addLabel: string;
  emptyLabel: string;
  removeLabel?: string;
}) {
  const list = useMemo(() => pairs, [pairs]);

  function update(index: number, key: string, value: string) {
    const next = list.slice();
    const existing = next[index];
    next[index] = { id: existing.id, key, value };
    onChange(next);
  }

  function remove(index: number) {
    const next = list.slice();
    next.splice(index, 1);
    onChange(next);
  }

  function add() {
    const next = list.slice();
    next.push({
      id: `kv-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      key: '',
      value: '',
    });
    onChange(next);
  }

  return (
    <div className="kv-editor">
      {list.length === 0 ? <p className="kv-editor-empty">{emptyLabel}</p> : null}
      {list.map((pair, index) => (
        <div key={pair.id} className="kv-editor-row">
          <input
            type="text"
            className="kv-editor-input"
            placeholder={keyPlaceholder}
            value={pair.key}
            onChange={(event) => update(index, event.target.value, pair.value)}
          />
          <input
            type="text"
            className="kv-editor-input"
            placeholder={valuePlaceholder}
            value={pair.value}
            onChange={(event) => update(index, pair.key, event.target.value)}
          />
          <IconButton
            className="kv-editor-icon-button"
            onClick={() => remove(index)}
            label={removeLabel}
          >
            <Trash2 size={14} />
          </IconButton>
        </div>
      ))}
      <button type="button" className="kv-editor-add" onClick={add}>
        <Plus size={14} />
        <span>{addLabel}</span>
      </button>
    </div>
  );
}

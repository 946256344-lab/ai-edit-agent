// 输出端口：选择目标编辑器并触发单向交付。
import type { EditorLinkerInfo } from '../lib/local-store'

type EditorOutputPortProps = {
  linkers: EditorLinkerInfo[]
  selectedId: string
  deliverLabel: string
  disabled: boolean
  busy: boolean
  onSelect: (editorId: string) => void
  onDeliver: () => void
}

export function EditorOutputPort({
  linkers,
  selectedId,
  deliverLabel,
  disabled,
  busy,
  onSelect,
  onDeliver,
}: EditorOutputPortProps) {
  const options = linkers.length
    ? linkers
    : [{ id: 'jianying', label: '剪映', summary: '', implemented: true, available: true, deliveryKind: 'dropInDraft' }]
  const selected = options.find((linker) => linker.id === selectedId)
  const unavailable = selected !== undefined && selected.implemented && !selected.available
  return (
    <div className="deliver-port">
      <label className="storyboard-version-picker">
        输出到
        <select
          value={selectedId}
          onChange={(event) => onSelect(event.target.value)}
          disabled={busy}
          aria-label="选择输出编辑器"
        >
          {options.map((linker) => (
            <option key={linker.id} value={linker.id} disabled={!linker.implemented}>
              {linker.implemented ? linker.label : `${linker.label}（即将支持）`}
            </option>
          ))}
        </select>
      </label>
      <button
        className="outline-button deliver-button"
        disabled={disabled || busy || unavailable}
        onClick={onDeliver}
        title={selected?.summary}
      >
        {deliverLabel} ↗
      </button>
      {unavailable && (
        <p className="deliver-unavailable-hint" role="alert">
          未检测到 {selected!.label} 草稿目录。请先打开 {selected!.label}，新建一个本地草稿后，
          再{' '}
          <button
            className="deliver-recheck-button"
            onClick={() => onSelect(selectedId)}
          >
            重新检测
          </button>
          。
        </p>
      )}
    </div>
  )
}

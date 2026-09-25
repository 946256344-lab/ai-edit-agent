// 输出端口：选择目标编辑器并触发单向交付。
import type { EditorLinkerInfo } from '../lib/local-store'
import { useI18n } from '../lib/i18n'

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
  const copy = useI18n().t.output
  const options = linkers.length
    ? linkers
    : [{ id: 'jianying', label: copy.jianying, summary: '', implemented: true, available: true, deliveryKind: 'dropInDraft' }]
  const selected = options.find((linker) => linker.id === selectedId)
  const unavailable = selected !== undefined && selected.implemented && !selected.available
  return (
    <div className="deliver-port">
      <label className="storyboard-version-picker">
        {copy.outputTo}
        <select
          value={selectedId}
          onChange={(event) => onSelect(event.target.value)}
          disabled={busy}
          aria-label={copy.selectEditor}
        >
          {options.map((linker) => (
            <option key={linker.id} value={linker.id} disabled={!linker.implemented}>
              {linker.implemented ? linker.label : copy.comingSoon(linker.label)}
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
          {copy.unavailableBefore(selected!.label)}
          <button
            className="deliver-recheck-button"
            onClick={() => onSelect(selectedId)}
          >
            {copy.recheck}
          </button>
          {copy.unavailableAfter}
        </p>
      )}
    </div>
  )
}

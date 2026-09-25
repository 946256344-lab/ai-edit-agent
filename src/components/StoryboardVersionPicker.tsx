// 故事版版本切换：列出本任务的全部故事版，局部改镜生成的派生版注明来源版本与改动的拍。
// 只读已加载的版本列表；来源版本或拍位置解析不到时退化显示，不额外请求后端。只有一版时不显示。
import type { StoryboardVersion } from '../lib/local-store'
import { useI18n, type Messages } from '../lib/i18n'

// 改动的拍超过这个数时只报数量，避免下拉项过长。
const MAX_LISTED_BEATS = 3

type StoryboardVersionPickerProps = {
  versions: StoryboardVersion[]
  selectedId: string | null
  onSelect: (storyboardVersionId: string) => void
}

function versionLabel(version: StoryboardVersion, versions: StoryboardVersion[], t: Messages): string {
  if (!version.derivedFromVersionId) return t.app.storyboardVersion(version.versionNumber)
  const parent = versions.find((candidate) => candidate.id === version.derivedFromVersionId)
  const changedBeatIds = version.changedBeatIds ?? []
  const positions = changedBeatIds.map((beatId) => version.beats.findIndex((beat) => beat.id === beatId) + 1)
  const beatPositions = positions.length > 0 && positions.length <= MAX_LISTED_BEATS && positions.every((position) => position > 0)
    ? [...positions].sort((a, b) => a - b)
    : null
  return t.app.derivedStoryboardVersion(version.versionNumber, parent?.versionNumber ?? null, beatPositions, changedBeatIds.length)
}

export function StoryboardVersionPicker({ versions, selectedId, onSelect }: StoryboardVersionPickerProps) {
  const { t } = useI18n()
  if (versions.length < 2) return null
  const labels = new Map(versions.map((version) => [version.id, versionLabel(version, versions, t)]))

  return (
    <label className="storyboard-version-picker">
      <select
        value={selectedId ?? ''}
        aria-label={t.app.storyboard}
        title={selectedId ? `${t.app.storyboard} ${labels.get(selectedId)}` : t.app.storyboard}
        onChange={(event) => {
          if (event.target.value) onSelect(event.target.value)
        }}
      >
        {versions.map((version) => (
          <option key={version.id} value={version.id}>
            {labels.get(version.id)}
          </option>
        ))}
      </select>
    </label>
  )
}

// 对齐 C:/tmp/opencut-classic/apps/web/src/selection — 仅类型占位，供 P3/P4 控制器复用
export type EditorSelectionSnapshot = {
  selectedElements: Array<{ trackId: string; elementId: string }>;
  selectedKeyframes: unknown[];
  keyframeSelectionAnchor: unknown;
  selectedMaskPoints: unknown;
};

export type EditorSelectionPatch = Partial<EditorSelectionSnapshot>;

export function emptySelection(): EditorSelectionSnapshot {
  return { selectedElements: [], selectedKeyframes: [], keyframeSelectionAnchor: null, selectedMaskPoints: null };
}

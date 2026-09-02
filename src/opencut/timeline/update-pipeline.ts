// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/update-pipeline.ts
// 本仓裁剪：retime/animation 派生仅保留 startTime/duration 约束，其余 pass-through。
import type { SceneTracks, TimelineElement } from "./types";
import { ZERO_MEDIA_TIME } from "../time/mediaTime";

export interface ElementUpdateContext {
  tracks: SceneTracks;
  trackId: string;
}

export function applyElementUpdate({
  element,
  patch,
  context,
}: {
  element: TimelineElement;
  patch: Partial<TimelineElement>;
  context: ElementUpdateContext;
}): TimelineElement {
  let next = {
    ...element,
    ...patch,
    params: { ...element.params, ...(patch.params ?? {}) },
  } as TimelineElement;

  if ("startTime" in patch) {
    const requested = (next.startTime as number) < (ZERO_MEDIA_TIME as number) ? ZERO_MEDIA_TIME : next.startTime;
    if (context.trackId !== context.tracks.main.id) {
      next = { ...next, startTime: requested };
    } else {
      const earliest = context.tracks.main.elements
        .filter((c) => c.id !== element.id)
        .reduce<TimelineElement | null>((a, c) => (!a || (c.startTime as number) < (a.startTime as number) ? c : a), null);
      next = {
        ...next,
        startTime: !earliest || (requested as number) <= (earliest.startTime as number) ? ZERO_MEDIA_TIME : requested,
      };
    }
  }

  return next;
}

// 对齐 apps/web/src/timeline/{element-snap-source,playhead-snap-source,bookmarks/snap-source}
import type { MediaTime } from "../time/mediaTime";
import type { SnapPoint } from "./snapping";
import type { SceneTracks } from "./types";
import { ZERO_MEDIA_TIME } from "../time/mediaTime";

export function getElementSnapPoints({
  tracks,
  excludeElementId,
}: {
  tracks: SceneTracks;
  excludeElementId?: string;
}): SnapPoint[] {
  const all = [...tracks.overlay, tracks.main, ...tracks.audio];
  const points: SnapPoint[] = [];
  for (const track of all) {
    for (const el of track.elements) {
      if (excludeElementId && el.id === excludeElementId) continue;
      points.push({ time: el.startTime, kind: "elementStart", sourceId: el.id });
      points.push({ time: ((el.startTime as number) + (el.duration as number)) as MediaTime, kind: "elementEnd", sourceId: el.id });
    }
  }
  return points;
}

export function getPlayheadSnapPoints({ playheadTime }: { playheadTime: MediaTime }): SnapPoint[] {
  return [{ time: playheadTime, kind: "playhead" }];
}

export function getBookmarkSnapPoints(bookmarks: Array<{ time: MediaTime }>): SnapPoint[] {
  return bookmarks.map((b) => ({ time: b.time, kind: "bookmark" }));
}

export function getZeroSnapPoint(): SnapPoint {
  return { time: ZERO_MEDIA_TIME, kind: "zero" };
}

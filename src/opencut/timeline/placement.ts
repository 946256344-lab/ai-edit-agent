// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/placement — 本仓 P3 裁剪版
// 仅保留 Studio 当前需要的「同一轨道内重排 + 叠加/音频自由放置」子集，其余策略 stub 为 firstAvailable。

import type { SceneTracks, TimelineTrack } from "./types";
import type { MediaTime } from "../time/mediaTime";
import { ZERO_MEDIA_TIME } from "../time/mediaTime";

export type PlacementTimeSpan = { startTime: MediaTime; duration: MediaTime; excludeElementId?: string };
export type PlacementSubject =
  | { elementType: string; trackType?: never }
  | { trackType: import("./types").TrackType; elementType?: never };
export type PlacementStrategy =
  | { type: "explicit"; trackId: string }
  | { type: "firstAvailable" }
  | { type: "preferIndex"; trackIndex: number; hoverDirection: "above" | "below"; verticalDragDirection?: "up" | "down" | null; createNewTrackOnly?: boolean }
  | { type: "aboveSource"; sourceTrackIndex: number }
  | { type: "newTrack"; position: "highest" | "default" };

export type PlacementResult =
  | { kind: "existingTrack"; trackId: string; trackIndex: number; trackType: import("./types").TrackType; adjustedStartTime?: MediaTime }
  | { kind: "newTrack"; trackType: import("./types").TrackType; insertIndex: number; insertPosition: "above" | "below" | null };

function canPlaceOnTrack(track: TimelineTrack, spans: PlacementTimeSpan[]): boolean {
  return spans.every(({ startTime, duration, excludeElementId }) => {
    const start = startTime as number;
    const end = start + (duration as number);
    return !track.elements.some((el) => {
      if (excludeElementId && el.id === excludeElementId) return false;
      const s = el.startTime as number;
      const e = s + (el.duration as number);
      return start < e && end > s;
    });
  });
}

export function resolveTrackPlacement(
  tracks: SceneTracks,
  subject: PlacementSubject,
  timeSpans: PlacementTimeSpan[],
  strategy: PlacementStrategy,
): PlacementResult | null {
  const ordered: TimelineTrack[] = [...tracks.overlay, tracks.main, ...tracks.audio];
  const trackType = (subject as { trackType?: import("./types").TrackType }).trackType ?? "video";
  if (strategy.type === "explicit") {
    const idx = ordered.findIndex((t) => t.id === strategy.trackId);
    if (idx < 0) return null;
    return { kind: "existingTrack", trackId: ordered[idx].id, trackIndex: idx, trackType: ordered[idx].type };
  }
  if (strategy.type === "firstAvailable") {
    const idx = ordered.findIndex((t) => t.type === trackType && canPlaceOnTrack(t, timeSpans));
    if (idx >= 0) return { kind: "existingTrack", trackId: ordered[idx].id, trackIndex: idx, trackType };
    return { kind: "newTrack", trackType, insertIndex: ordered.length, insertPosition: null };
  }
  if (strategy.type === "preferIndex") {
    const pref = ordered[strategy.trackIndex];
    if (pref && pref.type === trackType && canPlaceOnTrack(pref, timeSpans) && !strategy.createNewTrackOnly) {
      return { kind: "existingTrack", trackId: pref.id, trackIndex: strategy.trackIndex, trackType };
    }
    return { kind: "newTrack", trackType, insertIndex: strategy.trackIndex, insertPosition: strategy.hoverDirection };
  }
  if (strategy.type === "aboveSource") {
    const above = ordered[strategy.sourceTrackIndex - 1];
    if (above && above.type === trackType && canPlaceOnTrack(above, timeSpans)) {
      return { kind: "existingTrack", trackId: above.id, trackIndex: strategy.sourceTrackIndex - 1, trackType };
    }
    const idx = ordered.findIndex((t) => t.type === trackType && canPlaceOnTrack(t, timeSpans));
    if (idx >= 0) return { kind: "existingTrack", trackId: ordered[idx].id, trackIndex: idx, trackType };
    return { kind: "newTrack", trackType, insertIndex: ordered.length, insertPosition: null };
  }
  return { kind: "newTrack", trackType, insertIndex: ordered.length, insertPosition: null };
}

export function enforceMainTrackStart({
  tracks: _tracks,
  requestedStartTime,
}: {
  tracks: SceneTracks;
  targetTrackId?: string;
  requestedStartTime: MediaTime;
}): MediaTime {
  if ((requestedStartTime as number) < (ZERO_MEDIA_TIME as number)) return ZERO_MEDIA_TIME;
  return requestedStartTime;
}

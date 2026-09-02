// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/snapping/* + threshold
// 精选阈值 + 单轴 resolve，供 P1/P3 的移动/拉伸/播放头复用
import type { MediaTime } from "../time/mediaTime";
import { BASE_TIMELINE_PIXELS_PER_SECOND } from "./scale";
import { TICKS_PER_SECOND } from "../time/mediaTime";

export interface SnapPoint {
  time: MediaTime;
  kind?: string;
  sourceId?: string;
}
export interface SnapResult {
  snappedTime: MediaTime;
  snapPoint: SnapPoint | null;
  snapDistance: number;
}

export const DEFAULT_SNAP_THRESHOLD_PX = 10;

export function getSnapThresholdInTicks({ zoomLevel, snapThresholdPx = DEFAULT_SNAP_THRESHOLD_PX }: { zoomLevel: number; snapThresholdPx?: number }): number {
  const pps = BASE_TIMELINE_PIXELS_PER_SECOND * zoomLevel;
  return (snapThresholdPx / pps) * TICKS_PER_SECOND;
}

export function resolveTimelineSnap({
  targetTime,
  snapPoints,
  maxSnapDistance,
}: {
  targetTime: MediaTime;
  snapPoints: SnapPoint[];
  maxSnapDistance: number;
}): SnapResult {
  let best: SnapPoint | null = null;
  let bestDist = Infinity;
  for (const p of snapPoints) {
    const d = Math.abs((targetTime as number) - (p.time as number));
    if (d <= maxSnapDistance && d < bestDist) {
      bestDist = d;
      best = p;
    }
  }
  return { snappedTime: (best ? best.time : targetTime) as MediaTime, snapPoint: best, snapDistance: bestDist };
}

// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/pixel-utils.ts
import { BASE_TIMELINE_PIXELS_PER_SECOND } from "./scale";
import { TICKS_PER_SECOND } from "../time/mediaTime";
import type { MediaTime } from "../time/mediaTime";

export const TIMELINE_INDICATOR_LINE_WIDTH_PX = 2;

export function getTimelinePixelsPerSecond({ zoomLevel }: { zoomLevel: number }): number {
  return BASE_TIMELINE_PIXELS_PER_SECOND * zoomLevel;
}

export function timelineTimeToPixels({ time, zoomLevel }: { time: MediaTime; zoomLevel: number }): number {
  return ((time as number) / TICKS_PER_SECOND) * getTimelinePixelsPerSecond({ zoomLevel });
}

export function timelinePixelsToTime({ pixels, zoomLevel }: { pixels: number; zoomLevel: number }): MediaTime {
  return ((pixels / getTimelinePixelsPerSecond({ zoomLevel })) * TICKS_PER_SECOND) as MediaTime;
}

export function snapPixelToDeviceGrid({ pixel, devicePixelRatio }: { pixel: number; devicePixelRatio?: number }): number {
  const dpr = devicePixelRatio ?? (typeof window !== "undefined" ? window.devicePixelRatio : 1);
  const safe = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  return Math.round(pixel * safe) / safe;
}

export function timelineTimeToSnappedPixels({ time, zoomLevel, devicePixelRatio }: { time: MediaTime; zoomLevel: number; devicePixelRatio?: number }): number {
  return snapPixelToDeviceGrid({ pixel: timelineTimeToPixels({ time, zoomLevel }), devicePixelRatio });
}

export function getCenteredLineLeft({ centerPixel, lineWidthPx = TIMELINE_INDICATOR_LINE_WIDTH_PX }: { centerPixel: number; lineWidthPx?: number }): number {
  return centerPixel - lineWidthPx / 2;
}

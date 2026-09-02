// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/zoom-utils.ts
import { TIMELINE_ZOOM_MAX } from "./scale";
import { TICKS_PER_SECOND } from "../time/mediaTime";
import type { MediaTime } from "../time/mediaTime";

const PADDING_MAX_RATIO = 0.75;
const PADDING_MIN_RATIO = 0.15;
const PADDING_MIN_AT_ZOOM_PERCENT = 0.2;

export function getTimelineZoomMin({ duration, containerWidth }: { duration: MediaTime; containerWidth: number | null | undefined }): number {
  const safeDurationSeconds = Math.max((duration as number) / TICKS_PER_SECOND, 1);
  const safeContainerWidth = containerWidth ?? 1000;
  const availableWidth = safeContainerWidth * (1 - PADDING_MAX_RATIO);
  const zoomToFit = availableWidth / (safeDurationSeconds * 50);
  return Math.min(TIMELINE_ZOOM_MAX, zoomToFit);
}

export function getTimelinePaddingPx({ containerWidth, zoomLevel, minZoom }: { containerWidth: number; zoomLevel: number; minZoom: number }): number {
  const zoomPercent = getZoomPercent({ zoomLevel, minZoom });
  const t = Math.min(zoomPercent / PADDING_MIN_AT_ZOOM_PERCENT, 1);
  const ratio = PADDING_MAX_RATIO - (PADDING_MAX_RATIO - PADDING_MIN_RATIO) * t;
  return containerWidth * ratio;
}

export function getZoomPercent({ zoomLevel, minZoom }: { zoomLevel: number; minZoom: number }): number {
  return (zoomLevel - minZoom) / (TIMELINE_ZOOM_MAX - minZoom);
}

export function sliderToZoom({ sliderPosition, minZoom, maxZoom = TIMELINE_ZOOM_MAX }: { sliderPosition: number; minZoom: number; maxZoom?: number }): number {
  const clamped = Math.max(0, Math.min(1, sliderPosition));
  return minZoom * (maxZoom / minZoom) ** clamped;
}

export function zoomToSlider({ zoomLevel, minZoom, maxZoom = TIMELINE_ZOOM_MAX }: { zoomLevel: number; minZoom: number; maxZoom?: number }): number {
  const clampedZoom = Math.max(minZoom, Math.min(maxZoom, zoomLevel));
  return Math.log(clampedZoom / minZoom) / Math.log(maxZoom / minZoom);
}

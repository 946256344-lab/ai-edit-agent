// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/drag-utils.ts — 纯 MediaTime
import { BASE_TIMELINE_PIXELS_PER_SECOND } from "./scale";
import { mediaTime, TICKS_PER_SECOND } from "../time/mediaTime";
import type { MediaTime } from "../time/mediaTime";

export function getMouseTimeFromClientX({
  clientX,
  containerRect,
  zoomLevel,
  scrollLeft,
}: {
  clientX: number;
  containerRect: DOMRect;
  zoomLevel: number;
  scrollLeft: number;
}): MediaTime {
  const mouseX = clientX - containerRect.left + scrollLeft;
  const seconds = Math.max(0, mouseX / (BASE_TIMELINE_PIXELS_PER_SECOND * zoomLevel));
  return mediaTime(Math.round(seconds * TICKS_PER_SECOND));
}

export function clientDeltaToMediaTime(deltaPx: number, zoomLevel: number): MediaTime {
  const seconds = deltaPx / (BASE_TIMELINE_PIXELS_PER_SECOND * zoomLevel);
  return mediaTime(Math.round(seconds * TICKS_PER_SECOND));
}

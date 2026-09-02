// 精选移植 C:/tmp/opencut-classic/apps/web/src/timeline/controllers/zoom-controller.ts
import { TIMELINE_ZOOM_MAX } from "../scale";
import { timelineTimeToPixels } from "../pixel-utils";
import { zoomToSlider } from "../zoom-utils";
import type { MediaTime } from "../../time/mediaTime";

export interface ZoomConfig {
  minZoom: number;
  getTracksScrollEl: () => HTMLDivElement | null;
  getRulerScrollEl: () => HTMLDivElement | null;
  getCurrentPlayheadTime: () => MediaTime;
}

export class ZoomController {
  private readonly config: ZoomConfig;
  private zoom = 1;
  private prevZoom = 1;

  constructor(config: ZoomConfig, initialZoom?: number) {
    this.config = config;
    this.zoom = initialZoom ?? config.minZoom;
    this.prevZoom = this.zoom;
  }

  get zoomLevel(): number { return this.zoom; }

  setZoom(updater: number | ((prev: number) => number)): void {
    const raw = typeof updater === "function" ? updater(this.zoom) : updater;
    this.zoom = Math.max(this.config.minZoom, Math.min(TIMELINE_ZOOM_MAX, raw));
  }

  handleWheel(e: WheelEvent): void {
    const isZoom = e.ctrlKey || e.metaKey;
    if (!isZoom) return;
    const delta = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
    const capped = Math.sign(delta) * Math.min(Math.abs(delta), 30);
    const factor = Math.exp(-capped / 300);
    this.setZoom((prev) => prev * factor);
    this.applyLayout(this.zoom);
  }

  applyLayout(zoomLevel: number): void {
    if (this.prevZoom === zoomLevel) return;
    const scrollEl = this.config.getTracksScrollEl();
    if (!scrollEl) { this.prevZoom = zoomLevel; return; }
    const playhead = this.config.getCurrentPlayheadTime();
    const before = timelineTimeToPixels({ time: playhead, zoomLevel: this.prevZoom });
    const after = timelineTimeToPixels({ time: playhead, zoomLevel });
    const offset = before - scrollEl.scrollLeft;
    const next = after - offset;
    const maxScroll = scrollEl.scrollWidth - scrollEl.clientWidth;
    const clamped = Math.max(0, Math.min(maxScroll, next));
    scrollEl.scrollLeft = clamped;
    const ruler = this.config.getRulerScrollEl();
    if (ruler) ruler.scrollLeft = clamped;
    this.prevZoom = zoomLevel;
  }

  sliderToZoom(slider: number): number {
    const min = this.config.minZoom, max = TIMELINE_ZOOM_MAX;
    return min * (max / min) ** slider;
  }

  zoomToSlider(zoom: number): number {
    return zoomToSlider({ zoomLevel: zoom, minZoom: this.config.minZoom });
  }
}

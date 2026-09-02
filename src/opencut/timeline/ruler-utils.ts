// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/ruler-utils.ts — 本仓精选移植
// 原始导入 opencut-wasm / @/wasm / @/timeline/scale 已重定向为本地 opencut/time
import type { FrameRate } from "../time/frameRate";
import { frameRateToFloat } from "../time/frameRate";
import { BASE_TIMELINE_PIXELS_PER_SECOND } from "./scale";

const LABEL_FRAME_INTERVALS = [2, 3, 5, 10, 15] as const;
const TICK_FRAME_INTERVALS = [1, 2, 3, 5, 10, 15] as const;
const SECOND_MULTIPLIERS = [1, 2, 3, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600] as const;
const MIN_LABEL_SPACING_PX = 120;
const MIN_TICK_SPACING_PX = 18;

export interface RulerConfig {
  labelIntervalSeconds: number;
  tickIntervalSeconds: number;
}

export function getRulerConfig({ zoomLevel, fps }: { zoomLevel: number; fps: FrameRate }): RulerConfig {
  const fpsFloat = frameRateToFloat(fps);
  const pixelsPerSecond = BASE_TIMELINE_PIXELS_PER_SECOND * zoomLevel;
  const pixelsPerFrame = pixelsPerSecond / fpsFloat;
  const labelIntervalSeconds = findOptimalInterval({
    pixelsPerFrame,
    pixelsPerSecond,
    fps: fpsFloat,
    minSpacingPx: MIN_LABEL_SPACING_PX,
    frameIntervals: LABEL_FRAME_INTERVALS,
  });
  const raw = findOptimalInterval({
    pixelsPerFrame,
    pixelsPerSecond,
    fps: fpsFloat,
    minSpacingPx: MIN_TICK_SPACING_PX,
    frameIntervals: TICK_FRAME_INTERVALS,
  });
  return {
    labelIntervalSeconds,
    tickIntervalSeconds: ensureTickDividesLabel({
      tickIntervalSeconds: raw,
      labelIntervalSeconds,
      pixelsPerFrame,
      pixelsPerSecond,
      fps: fpsFloat,
    }),
  };
}

function ensureTickDividesLabel({
  tickIntervalSeconds,
  labelIntervalSeconds,
  pixelsPerFrame,
  pixelsPerSecond,
  fps,
}: {
  tickIntervalSeconds: number;
  labelIntervalSeconds: number;
  pixelsPerFrame: number;
  pixelsPerSecond: number;
  fps: number;
}): number {
  const labelFrames = Math.round(labelIntervalSeconds * fps);
  const tickFrames = Math.round(tickIntervalSeconds * fps);
  if (labelFrames % tickFrames === 0) return tickIntervalSeconds;
  for (const candidateFrames of TICK_FRAME_INTERVALS) {
    if (labelFrames % candidateFrames === 0) {
      if (pixelsPerFrame * candidateFrames >= MIN_TICK_SPACING_PX) return candidateFrames / fps;
    }
  }
  for (const candidateSeconds of SECOND_MULTIPLIERS) {
    const ratio = labelIntervalSeconds / candidateSeconds;
    if (Math.abs(ratio - Math.round(ratio)) < 0.0001) {
      if (pixelsPerSecond * candidateSeconds >= MIN_TICK_SPACING_PX) return candidateSeconds;
    }
  }
  return labelIntervalSeconds;
}

function findOptimalInterval({
  pixelsPerFrame,
  pixelsPerSecond,
  fps,
  minSpacingPx,
  frameIntervals,
}: {
  pixelsPerFrame: number;
  pixelsPerSecond: number;
  fps: number;
  minSpacingPx: number;
  frameIntervals: readonly number[];
}): number {
  for (const frameInterval of frameIntervals) {
    if (pixelsPerFrame * frameInterval >= minSpacingPx) return frameInterval / fps;
  }
  for (const secondMultiplier of SECOND_MULTIPLIERS) {
    if (pixelsPerSecond * secondMultiplier >= minSpacingPx) return secondMultiplier;
  }
  return 60;
}

export function shouldShowLabel({ time, labelIntervalSeconds }: { time: number; labelIntervalSeconds: number }): boolean {
  const epsilon = 0.0001;
  const remainder = time % labelIntervalSeconds;
  return remainder < epsilon || remainder > labelIntervalSeconds - epsilon;
}

export function formatRulerLabel({ timeInSeconds, fps }: { timeInSeconds: number; fps: FrameRate }): string {
  if (isSecondBoundary({ timeInSeconds })) return formatTimestamp({ timeInSeconds });
  const frameWithinSecond = getFrameWithinSecond({ timeInSeconds, fps: frameRateToFloat(fps) });
  return `${frameWithinSecond}f`;
}

function isSecondBoundary({ timeInSeconds }: { timeInSeconds: number }): boolean {
  const epsilon = 0.0001;
  const remainder = timeInSeconds % 1;
  return remainder < epsilon || remainder > 1 - epsilon;
}

function getFrameWithinSecond({ timeInSeconds, fps }: { timeInSeconds: number; fps: number }): number {
  return Math.round((timeInSeconds % 1) * fps);
}

function formatTimestamp({ timeInSeconds }: { timeInSeconds: number }): string {
  const totalSeconds = Math.round(timeInSeconds);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const mm = minutes.toString().padStart(2, "0");
  const ss = seconds.toString().padStart(2, "0");
  if (hours > 0) return `${hours}:${mm}:${ss}`;
  return `${mm}:${ss}`;
}

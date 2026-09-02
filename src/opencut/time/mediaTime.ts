// 对齐 C:/tmp/opencut-classic/rust/crates/time/src/media_time.rs + apps/web/src/wasm/media-time.ts
// 纯 TS 实现：整数 tick(120_000/s) + 帧对齐/磁吸所需的全部原语。

import type { FrameRate } from "./frameRate";
import { ticksPerFrame } from "./frameRate";

export const TICKS_PER_SECOND = 120_000 as const;

// MediaTime 为整数 tick 的不透明别名，运行时即 number。
export type MediaTime = number & { readonly __mediaTime: unique symbol };

export const ZERO_MEDIA_TIME = 0 as MediaTime;
export const ONE_TICK = 1 as MediaTime;

export function mediaTime(ticks: number): MediaTime {
  if (!Number.isInteger(ticks)) throw new Error(`mediaTime: expected integer ticks, got ${ticks}`);
  return ticks as MediaTime;
}

export function roundMediaTime(time: number): MediaTime {
  const mag = Math.round(Math.abs(time));
  if (mag === 0) return ZERO_MEDIA_TIME;
  return (time < 0 ? -mag : mag) as MediaTime;
}

export function mediaTimeFromSeconds(seconds: number): MediaTime | null {
  if (!Number.isFinite(seconds)) return null;
  const ticks = Math.round(seconds * TICKS_PER_SECOND);
  if (!Number.isSafeInteger(ticks)) return null;
  return ticks as MediaTime;
}

export function mediaTimeToSeconds(time: MediaTime): number {
  return (time as number) / TICKS_PER_SECOND;
}

export function addMediaTime(a: MediaTime, b: MediaTime): MediaTime {
  return ((a as number) + (b as number)) as MediaTime;
}

export function subMediaTime(a: MediaTime, b: MediaTime): MediaTime {
  return ((a as number) - (b as number)) as MediaTime;
}

export function minMediaTime(a: MediaTime, b: MediaTime): MediaTime {
  return ((a as number) < (b as number) ? a : b) as MediaTime;
}

export function maxMediaTime(a: MediaTime, b: MediaTime): MediaTime {
  return ((a as number) > (b as number) ? a : b) as MediaTime;
}

export function clampMediaTime(time: MediaTime, min: MediaTime, max: MediaTime): MediaTime {
  const t = time as number;
  if (t < (min as number)) return min;
  if (t > (max as number)) return max;
  return time;
}

function divEuclid(a: number, b: number): number {
  // JS 的 / 对于负数与 Rust 的 div_euclid 不一致，这里按 euclid 定义重实现
  // a = q*b + r, 0 <= r < |b|
  const q = Math.trunc(a / b);
  const r = a - q * b;
  if (r < 0) return b > 0 ? q - 1 : q + 1;
  return q;
}

function remEuclid(a: number, b: number): number {
  return a - divEuclid(a, b) * b;
}

export function toFrameFloor(time: MediaTime, rate: FrameRate): number | null {
  const tpf = ticksPerFrame(rate);
  if (tpf === null) return null;
  return divEuclid(time as number, tpf);
}

export function toFrameRound(time: MediaTime, rate: FrameRate): number | null {
  const tpf = ticksPerFrame(rate);
  if (tpf === null) return null;
  const r = remEuclid(time as number, tpf);
  const floor = divEuclid(time as number, tpf);
  if (r * 2 >= tpf) return floor + 1;
  return floor;
}

export function roundToFrame(time: MediaTime, rate: FrameRate): MediaTime | null {
  const frame = toFrameRound(time, rate);
  if (frame === null) return null;
  const tpf = ticksPerFrame(rate);
  if (tpf === null) return null;
  return (frame * tpf) as MediaTime;
}

export function floorToFrame(time: MediaTime, rate: FrameRate): MediaTime | null {
  const tpf = ticksPerFrame(rate);
  if (tpf === null) return null;
  return (divEuclid(time as number, tpf) * tpf) as MediaTime;
}

export function isFrameAligned(time: MediaTime, rate: FrameRate): boolean | null {
  const tpf = ticksPerFrame(rate);
  if (tpf === null) return null;
  return remEuclid(time as number, tpf) === 0;
}

export function lastFrameTime(duration: MediaTime, rate: FrameRate): MediaTime | null {
  if ((duration as number) <= 0) return ZERO_MEDIA_TIME;
  const lastTick = ((duration as number) - 1) as MediaTime;
  return floorToFrame(lastTick, rate);
}

export function snappedSeekTime(time: MediaTime, duration: MediaTime, rate: FrameRate): MediaTime | null {
  const snapped = roundToFrame(time, rate);
  if (snapped === null) return null;
  return clampMediaTime(snapped, ZERO_MEDIA_TIME, duration);
}

// —— 兼容旧数的辅助：ms <-> MediaTime ——
export function mediaTimeFromMs(ms: number): MediaTime {
  return mediaTimeFromSeconds(ms / 1000) ?? ZERO_MEDIA_TIME;
}

export function mediaTimeToMs(time: MediaTime): number {
  return Math.round(mediaTimeToSeconds(time) * 1000);
}

// 对齐 C:/tmp/opencut-classic/rust/crates/time/src/frame_rate.rs
// 本仓 P0 纯 TS 实现，不依赖 opencut-wasm binary。

export type FrameRate = { numerator: number; denominator: number };

export const FPS_23_976: FrameRate = { numerator: 24_000, denominator: 1_001 };
export const FPS_24: FrameRate = { numerator: 24, denominator: 1 };
export const FPS_25: FrameRate = { numerator: 25, denominator: 1 };
export const FPS_29_97: FrameRate = { numerator: 30_000, denominator: 1_001 };
export const FPS_30: FrameRate = { numerator: 30, denominator: 1 };
export const FPS_48: FrameRate = { numerator: 48, denominator: 1 };
export const FPS_50: FrameRate = { numerator: 50, denominator: 1 };
export const FPS_59_94: FrameRate = { numerator: 60_000, denominator: 1_001 };
export const FPS_60: FrameRate = { numerator: 60, denominator: 1 };
export const FPS_120: FrameRate = { numerator: 120, denominator: 1 };

export function isValidFrameRate(rate: FrameRate): boolean {
  return rate.numerator > 0 && rate.denominator > 0;
}

export function frameRateToFloat(rate: FrameRate): number {
  if (!isValidFrameRate(rate)) return 30;
  return rate.numerator / rate.denominator;
}

export function ticksPerFrame(rate: FrameRate): number | null {
  if (!isValidFrameRate(rate)) return null;
  const ticksPerSecond = 120_000;
  const num = ticksPerSecond * rate.denominator;
  const den = rate.numerator;
  if (num % den !== 0) return null;
  return num / den;
}

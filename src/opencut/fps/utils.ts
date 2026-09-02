// 对齐 C:/tmp/opencut-classic/apps/web/src/fps/utils.ts — 去 MediaAsset 依赖
import type { FrameRate } from "../time/frameRate";

const STANDARD_FRAME_RATES: Array<{ value: number; rate: FrameRate }> = [
  { value: 24_000 / 1_001, rate: { numerator: 24_000, denominator: 1_001 } },
  { value: 24, rate: { numerator: 24, denominator: 1 } },
  { value: 25, rate: { numerator: 25, denominator: 1 } },
  { value: 30_000 / 1_001, rate: { numerator: 30_000, denominator: 1_001 } },
  { value: 30, rate: { numerator: 30, denominator: 1 } },
  { value: 48, rate: { numerator: 48, denominator: 1 } },
  { value: 50, rate: { numerator: 50, denominator: 1 } },
  { value: 60_000 / 1_001, rate: { numerator: 60_000, denominator: 1_001 } },
  { value: 60, rate: { numerator: 60, denominator: 1 } },
  { value: 120, rate: { numerator: 120, denominator: 1 } },
];
const STANDARD_TOLERANCE = 0.01;

export function frameRateToFloat(rate: FrameRate): number {
  return rate.numerator / rate.denominator;
}

export function frameRatesEqual(a: FrameRate, b: FrameRate): boolean {
  return a.numerator === b.numerator && a.denominator === b.denominator;
}

export function floatToFrameRate(fps: number): FrameRate {
  const standard = STANDARD_FRAME_RATES.find((c) => Math.abs(fps - c.value) <= STANDARD_TOLERANCE);
  if (standard) return standard.rate;
  if (Number.isInteger(fps)) return { numerator: fps, denominator: 1 };
  const denom = 1_000_000;
  const num = Math.round(fps * denom);
  const g = gcd(num, denom);
  return { numerator: num / g, denominator: denom / g };
}

function gcd(a: number, b: number): number {
  let x = Math.abs(a);
  let y = Math.abs(b);
  while (y !== 0) {
    const r = x % y;
    x = y;
    y = r;
  }
  return x || 1;
}

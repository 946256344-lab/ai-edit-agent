// 对齐 C:/tmp/opencut-classic/apps/web/src/ripple/diff.ts — MediaTime 即 number(ticks)
import type { SceneTracks, TimelineElement, TimelineTrack } from "../timeline/types";
import type { RippleAdjustment } from "./apply";

interface Interval {
  startTime: number;
  endTime: number;
}

interface ElementSpan extends Interval {
  id: string;
}

export function computeRippleAdjustments({
  beforeTracks,
  afterTracks,
}: {
  beforeTracks: SceneTracks;
  afterTracks: SceneTracks;
}): RippleAdjustment[] {
  const beforeList: TimelineTrack[] = [...beforeTracks.overlay, beforeTracks.main, ...beforeTracks.audio];
  const afterList: TimelineTrack[] = [...afterTracks.overlay, afterTracks.main, ...afterTracks.audio];
  const afterById = new Map(afterList.map((t) => [t.id, t]));
  const allAfterIds = new Set(afterList.flatMap((t) => t.elements.map((e) => e.id)));
  return beforeList.flatMap((beforeTrack) =>
    computeTrackRippleAdjustments({
      trackId: beforeTrack.id,
      beforeElements: beforeTrack.elements as TimelineElement[],
      afterElements: (afterById.get(beforeTrack.id)?.elements ?? []) as TimelineElement[],
      allAfterElementIds: allAfterIds,
    }),
  );
}

function computeTrackRippleAdjustments({
  trackId,
  beforeElements,
  afterElements,
  allAfterElementIds,
}: {
  trackId: string;
  beforeElements: TimelineElement[];
  afterElements: TimelineElement[];
  allAfterElementIds: Set<string>;
}): RippleAdjustment[] {
  const beforeById = buildSpanMap(beforeElements);
  const afterById = buildSpanMap(afterElements);
  const { vacatedIntervals, joinedIntervals } = collectIntervals({ beforeElementsById: beforeById, afterElementsById: afterById, allAfterElementIds });
  const freed = subtractIntervalSets({ sourceIntervals: vacatedIntervals, overlappingIntervals: joinedIntervals });
  return buildAdjustments({ trackId, intervals: freed });
}

function buildSpanMap(elements: TimelineElement[]): Map<string, ElementSpan> {
  return new Map(elements.map((el) => [el.id, { id: el.id, startTime: el.startTime as number, endTime: (el.startTime as number) + (el.duration as number) }]));
}

function collectIntervals({
  beforeElementsById,
  afterElementsById,
  allAfterElementIds,
}: {
  beforeElementsById: Map<string, ElementSpan>;
  afterElementsById: Map<string, ElementSpan>;
  allAfterElementIds: Set<string>;
}): { vacatedIntervals: Interval[]; joinedIntervals: Interval[] } {
  const vacated: Interval[] = [];
  const joined: Interval[] = [];
  for (const before of beforeElementsById.values()) {
    const after = afterElementsById.get(before.id);
    if (!after) {
      if (!allAfterElementIds.has(before.id)) pushInterval({ intervals: vacated, startTime: before.startTime, endTime: before.endTime });
      continue;
    }
    if (before.endTime > after.endTime) pushInterval({ intervals: vacated, startTime: after.endTime, endTime: before.endTime });
  }
  for (const after of afterElementsById.values()) {
    if (beforeElementsById.has(after.id)) continue;
    pushInterval({ intervals: joined, startTime: after.startTime, endTime: after.endTime });
  }
  return { vacatedIntervals: normalizeIntervals({ intervals: vacated }), joinedIntervals: normalizeIntervals({ intervals: joined }) };
}

function buildAdjustments({ trackId, intervals }: { trackId: string; intervals: Interval[] }): RippleAdjustment[] {
  return intervals.flatMap((iv): RippleAdjustment[] => {
    const shiftAmount = iv.endTime - iv.startTime;
    if (shiftAmount <= 0) return [];
    return [{ trackId, afterTime: iv.endTime, shiftAmount }];
  });
}

function subtractIntervalSets({ sourceIntervals, overlappingIntervals }: { sourceIntervals: Interval[]; overlappingIntervals: Interval[] }): Interval[] {
  const a = normalizeIntervals({ intervals: sourceIntervals });
  const b = normalizeIntervals({ intervals: overlappingIntervals });
  return a.flatMap((src) => subtractSingleInterval({ sourceInterval: src, overlappingIntervals: b }));
}

function normalizeIntervals({ intervals }: { intervals: Interval[] }): Interval[] {
  const valid: Interval[] = [];
  for (const iv of intervals) pushInterval({ intervals: valid, startTime: iv.startTime, endTime: iv.endTime });
  const sorted = valid.sort((l, r) => l.startTime - r.startTime);
  if (sorted.length === 0) return [];
  const merged: Interval[] = [{ ...sorted[0] }];
  for (const iv of sorted.slice(1)) {
    const prev = merged[merged.length - 1];
    if (iv.startTime <= prev.endTime) {
      prev.endTime = Math.max(prev.endTime, iv.endTime);
      continue;
    }
    merged.push({ ...iv });
  }
  return merged;
}

function subtractSingleInterval({ sourceInterval, overlappingIntervals }: { sourceInterval: Interval; overlappingIntervals: Interval[] }): Interval[] {
  let rem: Interval[] = [{ ...sourceInterval }];
  for (const over of overlappingIntervals) {
    rem = rem.flatMap((r) => {
      if (over.endTime <= r.startTime || over.startTime >= r.endTime) return [r];
      const next: Interval[] = [];
      pushInterval({ intervals: next, startTime: r.startTime, endTime: over.startTime });
      pushInterval({ intervals: next, startTime: over.endTime, endTime: r.endTime });
      return next;
    });
    if (rem.length === 0) return [];
  }
  return rem;
}

function pushInterval({ intervals, startTime, endTime }: { intervals: Interval[]; startTime: number; endTime: number }): void {
  if (endTime <= startTime) return;
  intervals.push({ startTime, endTime });
}

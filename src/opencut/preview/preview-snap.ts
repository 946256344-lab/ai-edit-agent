// 逐行对齐 C:/tmp/opencut-classic/apps/web/src/preview/preview-snap.ts
export interface SnapLine { type: "horizontal" | "vertical"; position: number }
export const MIN_SCALE = 0.01;
export const SNAP_THRESHOLD_SCREEN_PIXELS = 8;
export interface SnapResult { snappedPosition: { x: number; y: number }; activeLines: SnapLine[] }
export type ScaleEdge = "left" | "right" | "top" | "bottom";
export interface ScaleEdgePreference { left?: boolean; right?: boolean; top?: boolean; bottom?: boolean }
function hasPreferredEdge({ preferredEdges, edge }: { preferredEdges?: ScaleEdgePreference; edge: ScaleEdge }): boolean { return preferredEdges?.[edge] === true; }
function pickClosestScaleCandidate<T extends { distance: number; edge: ScaleEdge }>({ candidates, preferredEdges }: { candidates: T[]; preferredEdges?: ScaleEdgePreference }): T | null {
  if (candidates.length === 0) return null;
  return candidates.reduce((best, cur) => {
    if (cur.distance < best.distance) return cur;
    if (cur.distance > best.distance) return best;
    const preferCur = hasPreferredEdge({ preferredEdges, edge: cur.edge });
    const preferBest = hasPreferredEdge({ preferredEdges, edge: best.edge });
    return preferCur && !preferBest ? cur : best;
  });
}
export function snapPosition({ proposedPosition, canvasSize, elementSize, rotation = 0, snapThreshold }: { proposedPosition: { x: number; y: number }; canvasSize: { width: number; height: number }; elementSize: { width: number; height: number }; rotation?: number; snapThreshold: { x: number; y: number } }): SnapResult {
  const centerX = 0, centerY = 0, left = -canvasSize.width / 2, right = canvasSize.width / 2, top = -canvasSize.height / 2, bottom = canvasSize.height / 2;
  const rotRad = (rotation * Math.PI) / 180, cosR = Math.abs(Math.cos(rotRad)), sinR = Math.abs(Math.sin(rotRad));
  const halfWidth = (elementSize.width * cosR + elementSize.height * sinR) / 2, halfHeight = (elementSize.width * sinR + elementSize.height * cosR) / 2;
  const activeLines: SnapLine[] = [];
  type Cand = { snappedPosition: number; line: SnapLine; distance: number };
  function closest(candidates: Cand[], threshold: number): Cand | null { const f = candidates.filter((c) => c.distance <= threshold); if (f.length === 0) return null; return f.reduce((a, b) => (b.distance < a.distance ? b : a)); }
  const xCandidates: Cand[] = []; for (const tx of [centerX, left, right]) { xCandidates.push({ snappedPosition: tx, line: { type: "vertical", position: tx }, distance: Math.abs(proposedPosition.x - tx) }); xCandidates.push({ snappedPosition: tx + halfWidth, line: { type: "vertical", position: tx }, distance: Math.abs(proposedPosition.x - halfWidth - tx) }); xCandidates.push({ snappedPosition: tx - halfWidth, line: { type: "vertical", position: tx }, distance: Math.abs(proposedPosition.x + halfWidth - tx) }); }
  const yCandidates: Cand[] = []; for (const ty of [centerY, top, bottom]) { yCandidates.push({ snappedPosition: ty, line: { type: "horizontal", position: ty }, distance: Math.abs(proposedPosition.y - ty) }); yCandidates.push({ snappedPosition: ty + halfHeight, line: { type: "horizontal", position: ty }, distance: Math.abs(proposedPosition.y - halfHeight - ty) }); yCandidates.push({ snappedPosition: ty - halfHeight, line: { type: "horizontal", position: ty }, distance: Math.abs(proposedPosition.y + halfHeight - ty) }); }
  const cx = closest(xCandidates, snapThreshold.x), cy = closest(yCandidates, snapThreshold.y);
  const x = cx?.snappedPosition ?? proposedPosition.x, y = cy?.snappedPosition ?? proposedPosition.y;
  if (cx) activeLines.push(cx.line); if (cy) activeLines.push(cy.line);
  return { snappedPosition: { x, y }, activeLines };
}
export interface ScaleSnapResult { snappedScale: number; activeLines: SnapLine[] }
export function snapScale({ proposedScale, position, baseWidth, baseHeight, rotation = 0, canvasSize, snapThreshold, preferredEdges }: { proposedScale: number; position: { x: number; y: number }; baseWidth: number; baseHeight: number; rotation?: number; canvasSize: { width: number; height: number }; snapThreshold: { x: number; y: number }; preferredEdges?: ScaleEdgePreference }): ScaleSnapResult {
  const centerX = 0, centerY = 0, left = -canvasSize.width / 2, right = canvasSize.width / 2, top = -canvasSize.height / 2, bottom = canvasSize.height / 2;
  const rotRad = (rotation * Math.PI) / 180, cosR = Math.abs(Math.cos(rotRad)), sinR = Math.abs(Math.sin(rotRad));
  const halfW = (baseWidth * cosR + baseHeight * sinR) / 2, halfH = (baseWidth * sinR + baseHeight * cosR) / 2;
  const leftEdge = position.x - halfW * proposedScale, rightEdge = position.x + halfW * proposedScale, topEdge = position.y - halfH * proposedScale, bottomEdge = position.y + halfH * proposedScale;
  interface Cand { scale: number; distance: number; lines: SnapLine[]; edge: ScaleEdge }
  const candidates: Cand[] = [];
  for (const t of [{ position: left, line: { type: "vertical" as const, position: left } }, { position: centerX, line: { type: "vertical" as const, position: centerX } }, { position: right, line: { type: "vertical" as const, position: right } }]) { const dL = Math.abs(leftEdge - t.position); if (dL <= snapThreshold.x) { const s = (position.x - t.position) / halfW; if (Math.abs(s) > MIN_SCALE) candidates.push({ scale: s, distance: dL, lines: [t.line], edge: "left" }); } const dR = Math.abs(rightEdge - t.position); if (dR <= snapThreshold.x) { const s = (t.position - position.x) / halfW; if (Math.abs(s) > MIN_SCALE) candidates.push({ scale: s, distance: dR, lines: [t.line], edge: "right" }); } }
  for (const t of [{ position: top, line: { type: "horizontal" as const, position: top } }, { position: centerY, line: { type: "horizontal" as const, position: centerY } }, { position: bottom, line: { type: "horizontal" as const, position: bottom } }]) { const dT = Math.abs(topEdge - t.position); if (dT <= snapThreshold.y) { const s = (position.y - t.position) / halfH; if (Math.abs(s) > MIN_SCALE) candidates.push({ scale: s, distance: dT, lines: [t.line], edge: "top" }); } const dB = Math.abs(bottomEdge - t.position); if (dB <= snapThreshold.y) { const s = (t.position - position.y) / halfH; if (Math.abs(s) > MIN_SCALE) candidates.push({ scale: s, distance: dB, lines: [t.line], edge: "bottom" }); } }
  const best = pickClosestScaleCandidate({ candidates, preferredEdges });
  if (!best) return { snappedScale: proposedScale, activeLines: [] };
  return { snappedScale: best.scale, activeLines: best.lines };
}
export interface RotationSnapResult { snappedRotation: number; isSnapped: boolean }
export function snapRotation({ proposedRotation }: { proposedRotation: number }): RotationSnapResult {
  const step = 90, nearest = Math.round(proposedRotation / step) * step;
  if (Math.abs(proposedRotation - nearest) <= 5) return { snappedRotation: nearest, isSnapped: true };
  return { snappedRotation: proposedRotation, isSnapped: false };
}

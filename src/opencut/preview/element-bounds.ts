// 精选对齐 C:/tmp/opencut-classic/apps/web/src/preview/element-bounds.ts
// 本仓不依赖 animation/rendering/media 实取，仅保留「画布中心+缩放+旋转→AABB」纯几何，供 P5 预览把手/命中
export interface ElementBounds { cx: number; cy: number; width: number; height: number; rotation: number }
export type Corner = "top-left" | "top-right" | "bottom-left" | "bottom-right";
export type Edge = "right" | "left" | "bottom";

export function getCornerPosition({ bounds, corner }: { bounds: ElementBounds; corner: Corner }): { x: number; y: number } {
  const hw = bounds.width / 2, hh = bounds.height / 2, rad = (bounds.rotation * Math.PI) / 180, cos = Math.cos(rad), sin = Math.sin(rad);
  const lx = corner === "top-left" || corner === "bottom-left" ? -hw : hw;
  const ly = corner === "top-left" || corner === "top-right" ? -hh : hh;
  return { x: bounds.cx + (lx * cos - ly * sin), y: bounds.cy + (lx * sin + ly * cos) };
}

export function getEdgeHandlePosition({ bounds, edge }: { bounds: ElementBounds; edge: Edge }): { x: number; y: number } {
  const hw = bounds.width / 2, hh = bounds.height / 2, rad = (bounds.rotation * Math.PI) / 180, cos = Math.cos(rad), sin = Math.sin(rad);
  const lx = edge === "right" ? hw : edge === "left" ? -hw : 0;
  const ly = edge === "bottom" ? hh : 0;
  return { x: bounds.cx + (lx * cos - ly * sin), y: bounds.cy + (lx * sin + ly * cos) };
}

export function hitTestBounds({ point, bounds }: { point: { x: number; y: number }; bounds: ElementBounds }): boolean {
  const rad = (-bounds.rotation * Math.PI) / 180, cos = Math.cos(rad), sin = Math.sin(rad);
  const dx = point.x - bounds.cx, dy = point.y - bounds.cy;
  const lx = dx * cos - dy * sin, ly = dx * sin + dy * cos;
  return Math.abs(lx) <= bounds.width / 2 && Math.abs(ly) <= bounds.height / 2;
}

export function boundsFromCanvasTransform({
  canvasWidth,
  canvasHeight,
  sourceWidth,
  sourceHeight,
  transform,
}: {
  canvasWidth: number; canvasHeight: number; sourceWidth: number; sourceHeight: number;
  transform: { scaleX: number; scaleY: number; position: { x: number; y: number }; rotate: number };
}): ElementBounds {
  const contain = Math.min(canvasWidth / sourceWidth, canvasHeight / sourceHeight);
  return { cx: canvasWidth / 2 + transform.position.x, cy: canvasHeight / 2 + transform.position.y, width: sourceWidth * contain * transform.scaleX, height: sourceHeight * contain * transform.scaleY, rotation: transform.rotate };
}

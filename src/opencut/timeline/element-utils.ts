// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/element-utils.ts — 本仓裁剪：去 effects/graphics/media/params 依赖
import {
  MASKABLE_ELEMENT_TYPES,
  RETIMABLE_ELEMENT_TYPES,
  VISUAL_ELEMENT_TYPES,
  type TimelineElement,
  type MaskableElement,
  type RetimableElement,
  type VisualElement,
  type VideoElement,
  type AudioElement,
} from "./types";

export function isVisualElement(element: TimelineElement): element is VisualElement {
  return (VISUAL_ELEMENT_TYPES as readonly string[]).includes(element.type);
}

export function isMaskableElement(element: TimelineElement): element is MaskableElement {
  return (MASKABLE_ELEMENT_TYPES as readonly string[]).includes(element.type);
}

export function isRetimableElement(element: TimelineElement): element is RetimableElement {
  return (RETIMABLE_ELEMENT_TYPES as readonly string[]).includes(element.type);
}

export function canElementHaveAudio(element: TimelineElement): element is AudioElement | VideoElement {
  return element.type === "audio" || element.type === "video";
}

export function hasMediaId(element: TimelineElement): boolean {
  return "mediaId" in element;
}

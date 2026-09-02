import type { TrackType, TimelineTrack } from "./types";

// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/track-capabilities.ts 的能力表
const TRACK_ELEMENT_TYPE: Record<TrackType, string[]> = {
  video: ["video", "image"],
  text: ["text"],
  audio: ["audio"],
  graphic: ["sticker", "graphic"],
  effect: ["effect"],
};

export function canInsertElementType(track: TimelineTrack, elementType: string): boolean {
  const allowed = TRACK_ELEMENT_TYPE[track.type];
  return allowed ? allowed.includes(elementType) : false;
}

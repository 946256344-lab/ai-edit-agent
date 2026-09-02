import type { SceneTracks, TScene } from "./types";
import { ZERO_MEDIA_TIME } from "../time/mediaTime";
import type { MediaTime } from "../time/mediaTime";

function uid(): string {
  return Math.random().toString(36).slice(2, 10);
}

export function createMainVideoTrack(id = "main"): SceneTracks["main"] {
  return { id, name: "Video", type: "video", elements: [], muted: false, hidden: false };
}

export function createEmptySceneTracks(): SceneTracks {
  return { overlay: [], main: createMainVideoTrack(), audio: [] };
}

export function createEmptyScene(name = "Scene"): TScene {
  const now = new Date();
  return {
    id: uid(),
    name,
    isMain: true,
    tracks: createEmptySceneTracks(),
    bookmarks: [],
    createdAt: now,
    updatedAt: now,
  };
}

export function sceneDuration(tracks: SceneTracks): MediaTime {
  let max: number = ZERO_MEDIA_TIME as number;
  const all = [...tracks.overlay, tracks.main, ...tracks.audio];
  for (const track of all) {
    for (const el of track.elements) {
      const end = (el.startTime as number) + (el.duration as number);
      if (end > max) max = end;
    }
  }
  return max as MediaTime;
}

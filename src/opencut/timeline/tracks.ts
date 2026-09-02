// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/tracks.ts
import type { SceneTracks, TimelineTrack } from "./types";

export function allTracks(tracks: SceneTracks): TimelineTrack[] {
  return [...tracks.overlay, tracks.main, ...tracks.audio];
}

export function findTrackById(tracks: SceneTracks, trackId: string): TimelineTrack | null {
  return allTracks(tracks).find((t) => t.id === trackId) ?? null;
}

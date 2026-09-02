// P7 存储桥：IndexedDB/OPFS → Tauri SQLite 适配层
// 不复刻 C:/tmp/opencut-classic/services/storage 的 31 次迁移，仅保证「SceneTracks/MediaTime ↔ TimelineVersion(mash)」来回无损
import type { SceneTracks } from "../timeline/types";
import { ZERO_MEDIA_TIME } from "../time/mediaTime";
import type { MediaTime } from "../time/mediaTime";
import { mediaTime } from "../time/mediaTime";
import type { TimelineVersion } from "../../lib/local-store";

function ticks(ms: number): MediaTime {
  return mediaTime(Math.round((ms / 1000) * 120_000));
}

export function timelineVersionToSceneTracks(timeline: TimelineVersion): SceneTracks {
  const videoElements = timeline.clips.map((clip) => ({
    id: `clip-${clip.shotIndex}`,
    name: clip.onScreenText ? clip.onScreenText.slice(0, 24) : `Clip ${clip.shotIndex}`,
    duration: ticks(clip.timelineEndMs - clip.timelineStartMs),
    startTime: ticks(clip.timelineStartMs),
    trimStart: ZERO_MEDIA_TIME,
    trimEnd: ZERO_MEDIA_TIME,
    sourceDuration: ticks(clip.sourceEndMs - clip.sourceStartMs),
    params: {},
    type: "video" as const,
    mediaId: clip.assetId,
    isSourceAudioEnabled: true,
    hidden: false,
  }));
  const textElementsByTrack = timeline.textTracks.map((track) =>
    track.cues.map((cue) => ({
      id: cue.id,
      name: cue.text.slice(0, 24) || "Text",
      duration: ticks(cue.endMs - cue.startMs),
      startTime: ticks(cue.startMs),
      trimStart: ZERO_MEDIA_TIME,
      trimEnd: ZERO_MEDIA_TIME,
      params: { content: cue.text, templateId: cue.templateId },
      type: "text" as const,
      hidden: false,
    })),
  );
  const overlayTracks: SceneTracks["overlay"] = textElementsByTrack.map((elements, i) => ({
    id: timeline.textTracks[i].id,
    name: timeline.textTracks[i].role,
    type: "text" as const,
    elements,
    hidden: false,
  }));
  const audioElements = [...(timeline.musicTracks ?? []), ...(timeline.voiceoverTracks ?? [])].flatMap((track) =>
    track.cues.map((cue) => ({
      id: cue.id,
      name: track.id,
      duration: ticks(cue.timelineEndMs - cue.timelineStartMs),
      startTime: ticks(cue.timelineStartMs),
      trimStart: ZERO_MEDIA_TIME,
      trimEnd: ZERO_MEDIA_TIME,
      sourceDuration: ticks(cue.sourceEndMs - cue.sourceStartMs),
      params: { volume: cue.volume },
      type: "audio" as const,
      sourceType: "upload" as const,
      mediaId: cue.assetId,
    })),
  );
  const audioTracks: SceneTracks["audio"] = audioElements.length
    ? [{ id: "audio-main", name: "Audio", type: "audio" as const, elements: audioElements as never, muted: false }]
    : [];
  return {
    overlay: overlayTracks,
    main: { id: "main", name: "Video", type: "video", elements: videoElements as never, muted: false, hidden: false },
    audio: audioTracks,
  };
}

// 对齐 C:/tmp/opencut-classic/apps/web/src/timeline/controllers/{drag-drop,element-interaction} — 本仓 P4b 精简版
// 仅接管「拖动中实时吸附」与「抬手落位」，不依赖 EditorCore/CommandManager
import { getSnapThresholdInTicks, resolveTimelineSnap } from "../snapping";
import { getElementSnapPoints, getPlayheadSnapPoints, getZeroSnapPoint } from "../snap-sources";
import { mediaTime } from "../../time/mediaTime";
import type { MediaTime } from "../../time/mediaTime";
import type { SceneTracks } from "../types";

export function resolveMoveSnap({
  targetStart,
  tracks,
  excludeElementId,
  playheadTime,
  zoomLevel,
  snapEnabled,
}: {
  targetStart: MediaTime;
  tracks: SceneTracks;
  excludeElementId: string;
  playheadTime: MediaTime | null;
  zoomLevel: number;
  snapEnabled: boolean;
}): { snappedStart: MediaTime; didSnap: boolean } {
  if (!snapEnabled) return { snappedStart: targetStart, didSnap: false };
  const points = [...getElementSnapPoints({ tracks, excludeElementId }), getZeroSnapPoint(), ...(playheadTime !== null ? getPlayheadSnapPoints({ playheadTime }) : [])];
  const maxDist = getSnapThresholdInTicks({ zoomLevel });
  const result = resolveTimelineSnap({ targetTime: targetStart, snapPoints: points, maxSnapDistance: maxDist });
  return { snappedStart: result.snappedTime, didSnap: result.snapPoint !== null };
}

export function resolveResizeSnap({
  targetEdge,
  tracks,
  excludeElementId,
  playheadTime,
  zoomLevel,
  snapEnabled,
}: {
  targetEdge: MediaTime;
  tracks: SceneTracks;
  excludeElementId: string;
  playheadTime: MediaTime | null;
  zoomLevel: number;
  snapEnabled: boolean;
}): { snappedEdge: MediaTime; didSnap: boolean } {
  return resolveMoveSnap({ targetStart: targetEdge, tracks, excludeElementId, playheadTime, zoomLevel, snapEnabled }) as unknown as { snappedEdge: MediaTime; didSnap: boolean };
}

// 将 Mash(毫秒) 结构临时投射为 SceneTracks(MediaTime ticks)，仅为取吸附点
export function mashToSceneTracksForSnap(mash: { tracks: Array<{ kind: string; clips: Array<{ id: string; timelineStartMs: number; timelineEndMs: number }> }> }): SceneTracks {
  const ticks = (ms: number) => mediaTime(Math.round((ms / 1000) * 120_000));
  const overlay: SceneTracks["overlay"] = [];
  const audio: SceneTracks["audio"] = [];
  let main: SceneTracks["main"] | null = null;
  for (const track of mash.tracks) {
    const elements = track.clips.map((c) => ({ id: c.id, name: c.id, duration: ticks(c.timelineEndMs - c.timelineStartMs), startTime: ticks(c.timelineStartMs), trimStart: 0 as MediaTime, trimEnd: 0 as MediaTime, params: {} })) as never[];
    if (track.kind === "video" && !main) main = { id: "main", name: "Video", type: "video", elements: elements as never, muted: false, hidden: false };
    else if (track.kind === "text") overlay.push({ id: track.kind + "-" + Math.random().toString(36).slice(2), name: "Text", type: "text", elements: elements as never, hidden: false } as never);
    else if (track.kind === "overlay") overlay.push({ id: "overlay-main", name: "Overlay", type: "video", elements: elements as never, muted: false, hidden: false } as never);
    else if (track.kind === "audio") audio.push({ id: track.kind + "-" + Math.random().toString(36).slice(2), name: "Audio", type: "audio", elements: elements as never, muted: false } as never);
  }
  return { overlay, main: main ?? { id: "main", name: "Video", type: "video", elements: [], muted: false, hidden: false }, audio };
}

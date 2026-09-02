export interface RippleAdjustment {
  trackId: string;
  afterTime: number;
  shiftAmount: number;
}

export function applyRippleAdjustments({
  tracks,
  adjustments,
}: {
  tracks: import("../timeline/types").SceneTracks;
  adjustments: RippleAdjustment[];
}): import("../timeline/types").SceneTracks {
  const byTrack = new Map(adjustments.map((a) => [a.trackId, a] as const));
  const shift = (startTime: number, trackId: string): number => {
    const adj = byTrack.get(trackId);
    if (!adj) return startTime;
    return (startTime as number) >= adj.afterTime ? ((startTime as number) - adj.shiftAmount) as unknown as typeof startTime : startTime;
  };
  const mapTrack = <T extends { id: string; elements: Array<{ startTime: number }> }>(track: T): T => {
    const adj = byTrack.get(track.id);
    if (!adj) return track;
    return { ...track, elements: track.elements.map((el) => ({ ...el, startTime: shift(el.startTime, track.id) })) } as T;
  };
  return {
    overlay: tracks.overlay.map((t) => mapTrack(t as never) as never),
    main: mapTrack(tracks.main as never) as never,
    audio: tracks.audio.map((t) => mapTrack(t as never) as never),
  };
}

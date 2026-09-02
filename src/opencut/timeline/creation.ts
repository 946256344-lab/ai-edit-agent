import { mediaTime, mediaTimeFromSeconds, TICKS_PER_SECOND } from "../time/mediaTime";
import type { MediaTime } from "../time/mediaTime";

export const DEFAULT_NEW_ELEMENT_DURATION: MediaTime = mediaTime(5 * TICKS_PER_SECOND);

export function toElementDurationTicks({ seconds }: { seconds: number | null | undefined }): MediaTime {
  if (seconds == null) return DEFAULT_NEW_ELEMENT_DURATION;
  return mediaTimeFromSeconds(seconds) ?? DEFAULT_NEW_ELEMENT_DURATION;
}

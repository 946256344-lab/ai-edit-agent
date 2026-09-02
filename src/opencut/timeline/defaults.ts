import { DEFAULT_NEW_ELEMENT_DURATION } from "./creation";
import { ZERO_MEDIA_TIME } from "../time/mediaTime";
import type { TextElement } from "./types";
import type { MediaTime } from "../time/mediaTime";

const defaultTextElement: Omit<TextElement, "id"> = {
  type: "text",
  name: "Text",
  duration: DEFAULT_NEW_ELEMENT_DURATION as MediaTime,
  startTime: ZERO_MEDIA_TIME,
  trimStart: ZERO_MEDIA_TIME,
  trimEnd: ZERO_MEDIA_TIME,
  params: {
    content: "Default text",
    fontSize: 15,
    fontFamily: "Arial",
    color: "#ffffff",
    textAlign: "center",
    fontWeight: "normal",
    fontStyle: "normal",
    textDecoration: "none",
    letterSpacing: 0,
    lineHeight: 1.2,
    opacity: 1,
  },
};

export const DEFAULTS = {
  text: { element: defaultTextElement },
};

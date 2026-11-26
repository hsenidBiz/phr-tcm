import { isCaptureMode } from "../dev/capture";
import { useExtrasUnlocked } from "../lib/extras";
import { snowShows, useNow, useSnowOff } from "../lib/snow";
import Snowfall from "./Snowfall";
import { useReducedMotion, useThemeIs } from "./decorShared";

/** Snow alone, for every theme except Halloween (none) and Christmas (its own
 * scenery draws it, in ChristmasCorner): in the (positioned) content column,
 * behind the page, clipped, taking no height and no click. Colours are the
 * --color-snow tokens, so a light theme gets a flake it can see. */
export default function SnowLayer() {
  const halloween = useThemeIs("halloween");
  const christmas = useThemeIs("christmas");
  const snowOff = useSnowOff();
  const unlocked = useExtrasUnlocked();
  const reducedMotion = useReducedMotion();
  const now = useNow();
  const theme = halloween ? "halloween" : christmas ? "christmas" : "other";

  if (!snowShows({ now, theme, snowOff, unlocked, capture: isCaptureMode(), reducedMotion })) return null;
  return <Snowfall className="snow-layer" />;
}

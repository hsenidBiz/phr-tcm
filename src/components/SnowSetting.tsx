import { isCaptureMode } from "../dev/capture";
import { useExtrasUnlocked } from "../lib/extras";
import { setSnowOff, snowCanFall, useNow, useSnowOff } from "../lib/snow";
import { useThemeIs } from "./decorShared";
import { SettingRow } from "./settings/SettingsCard";
import { Switch } from "./ui/switch";

/** The Snow row of Settings > Appearance. Its own component so the minute
 * clock it keeps (the row appears at midnight on 1 December) re-renders
 * only this row, not the whole Settings screen. Hidden when snow cannot
 * fall now, and under Halloween, which never snows. */
export default function SnowSetting() {
  const unlocked = useExtrasUnlocked();
  const snowOff = useSnowOff();
  const now = useNow();
  const halloween = useThemeIs("halloween");

  if (halloween || !snowCanFall({ now, unlocked, capture: isCaptureMode() })) return null;
  return (
    <SettingRow
      asLabel
      name="Snow"
      description="Falling snow in December. Turn it off any time."
      control={<Switch checked={!snowOff} onCheckedChange={(on) => setSnowOff(!on)} ariaLabel="Snow" />}
    />
  );
}

import { Kbd as XiodKey, KbdGroup } from "xiod-ui/kbd";

/** How each key is DRAWN. `mod` is resolved per platform below. */
const GLYPH: Record<string, string> = {
  ctrl: "⌃",
  alt: "⌥",
  shift: "⇧",
  enter: "↵",
  backspace: "⌫",
  escape: "Esc",
  tab: "⇥",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  plus: "+",
};

/** How each key is SPOKEN - glyphs mean nothing to a screen reader. */
const SPOKEN: Record<string, string> = {
  ctrl: "Control",
  alt: "Alt",
  shift: "Shift",
  enter: "Enter",
  backspace: "Backspace",
  escape: "Escape",
  tab: "Tab",
  up: "Up arrow",
  down: "Down arrow",
  left: "Left arrow",
  right: "Right arrow",
  plus: "Plus",
};

function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const hints = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  if (hints?.platform !== undefined) return /mac/i.test(hints.platform);
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform ?? "");
}

const glyph = (key: string, mac: boolean) =>
  key === "mod" ? (mac ? "⌘" : "Ctrl") : (GLYPH[key] ?? key.toUpperCase());
const spoken = (key: string, mac: boolean) =>
  key === "mod" ? (mac ? "Command" : "Control") : (SPOKEN[key] ?? key.toUpperCase());

/**
 * A keyboard shortcut, e.g. `<Kbd keys="mod+shift+m" />`: one XiodUI key cap
 * per key, announced in words ("Control + Shift + M"). It takes the same
 * `keys` strings the Astryx Kbd did, so Sidebar's `sectionShortcut` feeds it
 * unchanged. `mod` is Ctrl, or the Command key on a Mac.
 */
export function Kbd({ keys, className }: { keys: string; className?: string }) {
  const mac = isMac();
  const parts = keys.split("+").map((k) => k.trim().toLowerCase());
  return (
    <KbdGroup role="img" aria-label={parts.map((k) => spoken(k, mac)).join(" + ")} className={className}>
      {parts.map((k) => (
        <XiodKey key={k} aria-hidden="true">
          {glyph(k, mac)}
        </XiodKey>
      ))}
    </KbdGroup>
  );
}

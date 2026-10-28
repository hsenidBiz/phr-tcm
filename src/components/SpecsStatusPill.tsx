import { useSpecsPill } from "../lib/pwSpecs";
import StatusPill from "./StatusPill";

/** Whether the clone holds the rule files the script pipeline follows,
 * beside the Playwright title: green when it holds them all, amber when one
 * is missing or a change is being read in, red when the clone is not the
 * Playwright repo or could not be read. Shows nothing until the Playwright
 * screen has looked at a clone (`useSpecsWatch`). */
export default function SpecsStatusPill() {
  const pill = useSpecsPill();
  if (!pill) return null;
  return <StatusPill tone={pill.tone} label={pill.label} title={pill.why} ariaLabel={`Playwright pipeline: ${pill.label}. ${pill.why}`} />;
}

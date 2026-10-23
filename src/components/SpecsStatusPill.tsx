import { useSpecsPill } from "../lib/pwSpecs";
import StatusPill from "./StatusPill";

/** Whether the Playwright clone's spec files are found and read, beside the
 * Playwright title: green when they are, amber while a change is being read
 * in, red when there are none or they could not be read. Shows nothing until
 * the Playwright screen has looked at a clone (`useSpecsWatch`). */
export default function SpecsStatusPill() {
  const pill = useSpecsPill();
  if (!pill) return null;
  return <StatusPill tone={pill.tone} label={pill.label} title={pill.why} ariaLabel={`Playwright specs: ${pill.label}. ${pill.why}`} />;
}

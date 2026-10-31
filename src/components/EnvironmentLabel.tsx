import { activeEnvironment, useEnvironments } from "../lib/environments";
import EnvironmentPill from "./EnvironmentPill";

/** Which environment the data on a screen belongs to ("Scripts for QA"),
 * shown whatever the number of environments: Auto Run's scripts, areas and
 * templates are each environment's own, and a person must always be able to
 * tell whose they are looking at. Nothing until the list is known. */
export default function EnvironmentLabel({ prefix }: { prefix: string }) {
  const env = activeEnvironment(useEnvironments().data);
  if (!env) return null;
  return (
    <span role="note" aria-label="Environment">
      <EnvironmentPill name={`${prefix} ${env.name}`} />
    </span>
  );
}

// The script editor's Setup section: the fixture a script builds its data
// from, shown as it runs. The setup runs whenever the script runs.

import { useQuery } from "@tanstack/react-query";
import { loadSetupView, setupViewKey } from "./setupView";

export default function SetupSection({
  org,
  project,
  caseId,
}: {
  org: string;
  project: string;
  caseId: number;
}) {
  const view = useQuery({
    queryKey: setupViewKey(org, project, caseId),
    queryFn: () => loadSetupView(org, project, caseId),
    enabled: Boolean(org && project),
    retry: false,
  });

  const v = view.data;
  // A script with no setup has no section at all.
  if (!v) {
    return view.isError ? <p className="text-xs text-danger">{view.error.message}</p> : null;
  }

  return (
    <section aria-label="Setup" className="space-y-1">
      <span className="text-xs font-medium text-muted">Setup</span>
      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
        <dt className="text-muted">Fixture</dt>
        <dd className="break-words text-text">{v.fixture_name}</dd>
        <dt className="text-muted">Runs as</dt>
        <dd className="break-words text-text">{v.account}</dd>
      </dl>
      {v.steps.length > 0 && (
        <ol aria-label="Setup steps" className="space-y-0.5 text-xs">
          {v.steps.map((line, i) => (
            <li key={`${i}-${line}`} className="id-mono break-words text-text">
              {line}
            </li>
          ))}
        </ol>
      )}
      {v.creates.length > 0 && (
        <p className="text-xs text-text">
          <span className="text-muted">Creates </span>
          {v.creates.join(", ")}
        </p>
      )}
    </section>
  );
}

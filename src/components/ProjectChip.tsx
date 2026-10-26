// A small pill naming the project an item belongs to. Shown beside items
// only where the list spans more than one project (lib/projectScope.ts's
// spansProjects), so a single-project list stays as quiet as before.
export default function ProjectChip({ project }: { project: string }) {
  return (
    <span
      className="shrink-0 rounded-full bg-surface-2 px-1.5 py-0.5 text-[10px] text-muted"
      title={`Project: ${project}`}
    >
      <span className="label-trim">{project}</span>
    </span>
  );
}

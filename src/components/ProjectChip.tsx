// A small pill naming the project an item belongs to. Shown beside items
// only where the list spans more than one project (lib/projectScope.ts's
// spansProjects), so a single-project list stays as quiet as before.
// inline-block: its label is a block (`.label-trim`), and a block inside a
// plain inline span splits the pill into pieces wherever the chip sits in
// running text (a group heading) rather than in a flex row.
export default function ProjectChip({ project }: { project: string }) {
  return (
    <span
      className="inline-block shrink-0 rounded-full bg-surface-2 px-1.5 py-0.5 text-[10px] text-muted"
      title={`Project: ${project}`}
    >
      <span className="sr-only">Project: </span>
      <span className="label-trim">{project}</span>
    </span>
  );
}

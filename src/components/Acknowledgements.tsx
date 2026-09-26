/** Settings' closing section: who built the app, who tested it, and who
 *  made it possible for the teams to use it. Plain text, no controls. */

const DEVELOPER = "Avin Alwis";

const TESTERS = [
  "Ishani Dasanayake",
  "Naveen Warnakulasuriya",
  "Sachila Manamperi",
  "Vishwa Warnakulasuriya",
  "Hansani Gunasekara",
];

const THANKS = {
  name: "Ayub Sourjah",
  for: "for the feedback, and for the opportunity to bring Test Case Manager to the teams.",
};

export default function Acknowledgements() {
  return (
    <section aria-labelledby="acknowledgements-title" className="space-y-3">
      <h2 id="acknowledgements-title" className="text-sm font-semibold text-text">
        Acknowledgements
      </h2>
      <p className="text-sm text-muted">Built by the Innovation Team.</p>
      <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
        <dt className="text-muted">Developer</dt>
        <dd className="text-text">{DEVELOPER}</dd>
        <dt className="text-muted">Testers</dt>
        <dd>
          <ul className="space-y-0.5 text-text">
            {TESTERS.map((name) => (
              <li key={name}>{name}</li>
            ))}
          </ul>
        </dd>
      </dl>
      <div className="rounded-md border border-border bg-surface-2 px-3 py-2.5 text-sm">
        <p className="text-xs font-semibold uppercase tracking-wide text-muted">Special thanks</p>
        <p className="mt-1 text-muted">
          <span className="font-medium text-text">{THANKS.name}</span>, {THANKS.for}
        </p>
      </div>
    </section>
  );
}

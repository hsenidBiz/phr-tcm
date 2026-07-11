import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { commands, type SubmitItemResult, type TestCase } from "../bindings";

const inputCls =
  "rounded-md border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none";
const btnCls =
  "rounded-md border border-neutral-700 px-3 py-1.5 text-sm hover:border-blue-500 disabled:opacity-40";

function parseStepsText(text: string) {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .map((line) => {
      const [action, expected = ""] = line.split("=>");
      return { action: action.trim(), expected: expected.trim() };
    })
    .filter((s) => s.action);
}

export default function QueuePanel({
  org,
  project,
  pbiId,
}: {
  org: string;
  project: string;
  pbiId: number;
}) {
  const qc = useQueryClient();
  const [queue, setQueue] = useState<TestCase[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [results, setResults] = useState<SubmitItemResult[] | null>(null);

  const [title, setTitle] = useState("");
  const [stepsText, setStepsText] = useState("");
  const [tags, setTags] = useState("");
  const [status, setStatus] = useState("Not Automated");

  function addManual() {
    const steps = parseStepsText(stepsText);
    if (!title.trim() || steps.length === 0) return;
    setQueue((q) => [
      ...q,
      {
        title: title.trim(),
        steps,
        tags: tags.trim(),
        automation_status: status,
        module_value: "",
        preconditions: "",
        update_id: null,
      },
    ]);
    setTitle("");
    setStepsText("");
    setTags("");
  }

  const importFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "Import", extensions: ["xlsx", "csv", "json"] }],
      });
      if (typeof path !== "string") return null;
      const r = await commands.parseImportFile(path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (data) => {
      if (!data) return;
      setQueue((q) => [...q, ...data.cases]);
      setWarnings(data.warnings);
    },
  });

  const saveTemplate = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: "test-case-template.xlsx",
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path) return;
      const r = await commands.writeTemplate(path);
      if (r.status === "error") throw new Error(r.error);
    },
  });

  const exportQueue = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: "test-case-queue.xlsx",
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path) return;
      const r = await commands.exportQueue(path, queue);
      if (r.status === "error") throw new Error(r.error);
    },
  });

  const submit = useMutation({
    mutationFn: async () => {
      const r = await commands.submitQueue(org, project, pbiId, queue);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (data) => {
      setResults(data);
      // Keep only items that failed so the user can fix and retry.
      const failed = new Set(data.filter((r) => r.action === "failed").map((r) => r.index));
      setQueue((q) => q.filter((_, i) => failed.has(i)));
      qc.invalidateQueries({ queryKey: ["pbi-tcs", org, pbiId] });
    },
  });

  return (
    <section className="space-y-3 rounded-md border border-neutral-800 p-4">
      <h2 className="text-sm font-semibold text-neutral-300">
        Queue for PBI #{pbiId} ({queue.length} queued)
      </h2>

      <div className="grid gap-2 md:grid-cols-2">
        <div className="space-y-2">
          <input
            className={inputCls + " w-full"}
            placeholder="Test case title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
          <textarea
            className={inputCls + " h-24 w-full font-mono text-xs"}
            placeholder={"One step per line:\naction => expected result"}
            value={stepsText}
            onChange={(e) => setStepsText(e.target.value)}
          />
          <div className="flex gap-2">
            <input
              className={inputCls + " flex-1"}
              placeholder="Tags (semicolon-separated)"
              value={tags}
              onChange={(e) => setTags(e.target.value)}
            />
            <select className={inputCls} value={status} onChange={(e) => setStatus(e.target.value)}>
              <option>Not Automated</option>
              <option>Planned</option>
            </select>
            <button className={btnCls} onClick={addManual}>
              Add to queue
            </button>
          </div>
        </div>
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            <button className={btnCls} disabled={importFile.isPending} onClick={() => importFile.mutate()}>
              Import file...
            </button>
            <button className={btnCls} onClick={() => saveTemplate.mutate()}>
              Save template...
            </button>
            <button className={btnCls} disabled={queue.length === 0} onClick={() => exportQueue.mutate()}>
              Export queue...
            </button>
          </div>
          {importFile.isError && (
            <p className="text-sm text-red-400">{importFile.error.message}</p>
          )}
          {warnings.length > 0 && (
            <ul className="max-h-24 space-y-0.5 overflow-y-auto text-xs text-amber-400">
              {warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          )}
        </div>
      </div>

      {queue.length > 0 && (
        <ul className="space-y-1">
          {queue.map((tc, i) => (
            <li
              key={i}
              className="flex items-center justify-between rounded-md border border-neutral-800 px-3 py-1.5 text-sm"
            >
              <span>
                {tc.update_id != null && (
                  <span className="mr-2 rounded bg-amber-900/60 px-1.5 py-0.5 text-xs text-amber-300">
                    UPDATE #{tc.update_id}
                  </span>
                )}
                {tc.title}
                <span className="ml-2 text-xs text-neutral-500">{tc.steps.length} steps</span>
              </span>
              <button
                className="text-xs text-neutral-500 hover:text-red-400"
                onClick={() => setQueue((q) => q.filter((_, j) => j !== i))}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="flex items-center gap-3">
        <button
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-500 disabled:opacity-50"
          disabled={queue.length === 0 || submit.isPending}
          onClick={() => submit.mutate()}
        >
          {submit.isPending ? "Creating..." : `Create ${queue.length} test case${queue.length === 1 ? "" : "s"}`}
        </button>
        {submit.isError && <p className="text-sm text-red-400">{submit.error.message}</p>}
      </div>

      {results && (
        <ul className="space-y-0.5 text-sm">
          {results.map((r) => (
            <li key={r.index} className={r.action === "failed" ? "text-red-400" : "text-green-400"}>
              {r.action === "created" && `Created #${r.id}: ${r.title}`}
              {r.action === "updated" && `Updated #${r.id}: ${r.title}`}
              {r.action === "failed" && `Failed: ${r.title} - ${r.error}`}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

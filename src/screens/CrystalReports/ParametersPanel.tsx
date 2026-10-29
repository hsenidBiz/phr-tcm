// The open report's parameters, with a field for each. The values typed here
// draw the next preview and nothing else: they are not kept anywhere, and
// not written into the report. Another report starts with empty fields (the
// screen gives this panel a new key per path).

import { useCallback, useEffect, useState } from "react";
import { commands, type ReportParameter } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import Combobox from "../../components/ui/combobox";
import { Input } from "../../components/ui/input";
import { IconPreview } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

/** What to type, by the parameter's kind. */
const HINTS: Record<string, string> = {
  Number: "A number",
  Currency: "A number",
  Date: "YYYY-MM-DD",
  Time: "HH:MM:SS",
  DateTime: "YYYY-MM-DDTHH:MM:SS",
  Boolean: "true or false",
};

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function ParametersPanel({ onPreviewed }: { path: string; onPreviewed: () => void }) {
  const [params, setParams] = useState<ReportParameter[] | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    unwrapStr(commands.crystalParameters())
      .then((list) => live && setParams(Array.isArray(list) ? list : []))
      .catch(() => live && setParams([]));
    return () => {
      live = false;
    };
  }, []);

  const preview = useCallback(async () => {
    if (!params) return;
    setBusy(true);
    setError(null);
    try {
      const filled = params
        .map((p) => ({ name: p.name, value: (values[p.name] ?? "").trim() }))
        .filter((v) => v.value !== "");
      await unwrapStr(commands.crystalDraw(filled));
      onPreviewed();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }, [params, values, onPreviewed]);

  if (!params || params.length === 0) return null;
  return (
    <section aria-label="Parameters" className="shrink-0 rounded-lg border border-border bg-surface px-4 py-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text">Parameters</h2>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void preview()}>
          <IconPreview aria-hidden />
          Preview with these values
        </Button>
      </div>
      <div className="grid gap-x-4 gap-y-2 sm:grid-cols-2 lg:grid-cols-3">
        {params.map((p) => {
          const hint = HINTS[p.value_kind] ?? "Text";
          const value = values[p.name] ?? "";
          const set = (v: string) => setValues((all) => ({ ...all, [p.name]: v }));
          return (
            <div key={p.name} className="flex min-w-0 flex-col gap-1">
              <div className="flex min-w-0 flex-wrap items-center gap-x-2">
                <span className="truncate text-xs font-medium text-text">{p.name}</span>
                {(p.optional_prompt || p.allow_null_value) && <Badge>Optional</Badge>}
              </div>
              {p.prompt_text && <span className="text-xs text-muted">{p.prompt_text}</span>}
              {p.choices.length > 0 ? (
                <Combobox
                  ariaLabel={p.name}
                  value={value}
                  onChange={set}
                  items={p.choices.map((c) => ({
                    value: c.value,
                    label: c.description ? `${c.value} - ${c.description}` : c.value,
                  }))}
                  placeholder={hint}
                  allowCustom
                />
              ) : (
                <Input
                  aria-label={p.name}
                  className="py-1.5"
                  value={value}
                  placeholder={hint}
                  onChange={(e) => set(e.target.value)}
                />
              )}
            </div>
          );
        })}
      </div>
      {error && (
        <p role="alert" className="mt-2 text-xs text-danger">
          {error}
        </p>
      )}
    </section>
  );
}

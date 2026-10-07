import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { commands, type WritingStyle } from "../bindings";
import { Button } from "./ui/button";
import { Textarea } from "./ui/input";
import { Switch } from "./ui/switch";
import { IconConfirm, IconImport, IconUndo } from "../lib/actionIcons";
import { unwrapStr } from "../lib/ipc";
import { toast } from "../lib/toast";
import { WRITING_STYLE_QUERY } from "../lib/writingStyle";

/** The AI Bridge tab's Writing style card: the person's own rules for how
 * test cases are designed, which the writing guide carries instead of its
 * standard granularity and edge-case sections while the switch is on.
 *
 * The switch saves at once, like the tab's other switches, while the text
 * is as saved. Once the text has unsaved edits the switch joins them in
 * the draft: Save keeps both, Discard changes goes back to what is saved. */
export function WritingStyleCard() {
  const qc = useQueryClient();
  const saved = useQuery({ queryKey: WRITING_STYLE_QUERY, queryFn: () => commands.writingStyleGet() });
  // Null while the editor shows exactly what is saved.
  const [draft, setDraft] = useState<WritingStyle | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const base = typeof saved.data?.text === "string" ? saved.data : null;
  const dirty = draft !== null && base !== null && (draft.enabled !== base.enabled || draft.text !== base.text);
  const textEdited = draft !== null && base !== null && draft.text !== base.text;
  // The switch waiting on Save, because the text has edits to go with it.
  const switchWaits = textEdited && draft.enabled !== base.enabled;

  const edit = (next: Partial<WritingStyle>) => {
    if (!shown) return;
    setDraft({ ...shown, ...next });
    setProblem(null);
  };

  const save = useMutation({
    mutationFn: (style: WritingStyle) => unwrapStr(commands.writingStyleSave(style)),
    onSuccess: (_, style) => {
      qc.setQueryData(WRITING_STYLE_QUERY, style);
      setDraft(null);
      setProblem(null);
      toast.success("Writing style saved.");
    },
    onError: (e) => setProblem(e.message),
  });

  // While a switch-only save is on its way, the switch shows where it is going.
  const shown = draft ?? (save.isPending && save.variables ? save.variables : base);

  const flip = (on: boolean) => {
    if (!base) return;
    if (textEdited) {
      edit({ enabled: on });
      return;
    }
    setDraft(null);
    setProblem(null);
    save.mutate({ enabled: on, text: base.text });
  };

  const discard = () => {
    setDraft(null);
    setProblem(null);
  };

  const upload = async () => {
    const picked = await open({
      multiple: false,
      filters: [{ name: "Markdown documents", extensions: ["md", "markdown"] }],
    });
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (typeof path !== "string") return;
    try {
      const text = await unwrapStr(commands.writingStyleReadFile(path));
      edit({ text });
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <h2 className="text-sm font-semibold text-text">Writing style</h2>
      <p className="text-xs text-muted">
        Your own rules for how the assistant designs test cases, in Markdown. While it is on, the
        writing guide carries them instead of its standard granularity and edge-case sections.
      </p>
      {shown && (
        <>
          <div className="flex items-center justify-between gap-2 rounded-md border border-border/60 p-2">
            <span className="text-xs font-medium text-muted">Use my writing style</span>
            <Switch
              ariaLabel="Use my writing style"
              checked={shown.enabled}
              disabled={save.isPending}
              onCheckedChange={flip}
            />
          </div>
          {switchWaits && <p className="text-xs text-muted">Save to apply the switch with your edits.</p>}
          <Textarea
            aria-label="Writing style"
            rows={16}
            spellCheck={false}
            className="h-80 w-full font-mono text-xs"
            value={shown.text}
            onChange={(e) => edit({ text: e.target.value })}
          />
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="outline" disabled={save.isPending} onClick={() => void upload()}>
              <IconImport aria-hidden />
              Upload .md
            </Button>
            <Button
              size="sm"
              disabled={!dirty || save.isPending}
              onClick={() => draft && save.mutate(draft)}
            >
              <IconConfirm aria-hidden />
              {save.isPending ? "Saving" : "Save"}
            </Button>
            <Button size="sm" variant="outline" disabled={!dirty || save.isPending} onClick={discard}>
              <IconUndo aria-hidden />
              Discard changes
            </Button>
          </div>
          {problem && (
            <p role="alert" className="text-xs text-danger">
              {problem}
            </p>
          )}
        </>
      )}
      <p className="text-[11px] text-faint">
        Takes effect the next time the assistant reads the writing guide. The case format and import
        rules always apply.
      </p>
    </section>
  );
}

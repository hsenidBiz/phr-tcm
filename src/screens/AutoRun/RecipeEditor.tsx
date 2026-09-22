// The project's one sign-in recipe, as JSON. JSON on purpose, like the
// script editor: the format has to be proven by hand before anything
// friendlier is built on it.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type Quirk, type SignInRecipe_Deserialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

const PLACEHOLDER = `{
  "start_url": "https://hr.example.internal/",
  "steps": [
    { "kind": "fill", "selector": { "role": "textbox", "name": "Username" }, "value": "{{username}}" },
    { "kind": "fill", "selector": { "css": "input[type=password]" }, "value": "{{password}}" },
    { "kind": "click", "selector": { "role": "button", "name": "Login" } },
    { "kind": "when_visible", "selector": { "role": "button", "name": "Continue here" }, "within_ms": 5000,
      "then": [ { "kind": "click", "selector": { "role": "button", "name": "Continue here" } } ] }
  ],
  "signed_in": { "css": "#sidebar-toggle-menu" },
  "allowed_origins": [],
  "session_minutes": 480
}`;

/** Case- and whitespace-insensitive, matching the Rust side's own `normalized()`. */
function normalized(s: string): string {
  return s.split(/\s+/).join(" ").toLowerCase();
}

/**
 * The quirks box's current lines, matched back against what was loaded so
 * an unchanged line keeps its original author and timestamp. A line that
 * matches nothing on the loaded list is new, written by the person editing
 * this dialog right now. A line that repeats one already emitted (same
 * text, any case or spacing) collapses to its first occurrence - the box
 * is a set of facts, not a log of how many times each was typed.
 */
function linesToQuirks(text: string, loaded: Quirk[]): Quirk[] {
  const pool = [...loaded];
  const now = String(Date.now());
  const seen = new Set<string>();
  const out: Quirk[] = [];
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line === "") continue;
    const key = normalized(line);
    if (seen.has(key)) continue;
    seen.add(key);
    const i = pool.findIndex((q) => q.text === line);
    if (i >= 0) {
      out.push(pool[i]);
      pool.splice(i, 1);
    } else {
      out.push({ text: line, by: "person", at: now });
    }
  }
  return out;
}

export default function RecipeEditor({ org, project, onClose }: { org: string; project: string; onClose: () => void }) {
  const qc = useQueryClient();
  const existing = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: () => unwrapStr(commands.autoRunLoadRecipe(org, project)),
    retry: false,
  });
  const existingQuirks = useQuery({
    queryKey: ["autorun-quirks", org, project],
    queryFn: () => unwrapStr(commands.autoRunLoadQuirks(org, project)),
    retry: false,
  });
  const [text, setText] = useState<string | null>(null);
  const [quirksText, setQuirksText] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const value = text ?? (existing.data ? JSON.stringify(existing.data, null, 2) : "");
  const quirksValue = quirksText ?? (existingQuirks.data ?? []).map((q) => q.text).join("\n");
  const blocked = existing.isLoading || existing.isError || existingQuirks.isLoading || existingQuirks.isError;
  const recipeEmpty = value.trim() === "";
  const quirksEmpty = quirksValue.trim() === "";

  // `recipe` is `null` for a project with no recipe yet, whose box is left
  // empty on purpose - that box saves only the quirks, never an
  // `auto_run_save_recipe` call with nothing behind it.
  const save = useMutation({
    mutationFn: async (recipe: SignInRecipe_Deserialize | null) => {
      // The recipe is saved first; a refusal here (a bad selector, a
      // missing address) must leave the quirks box exactly as typed and
      // never write it - saving a fact about the app is not consolation
      // for a recipe that did not actually save.
      if (recipe) {
        await unwrapStr(commands.autoRunSaveRecipe(org, project, recipe));
      }
      await unwrapStr(commands.autoRunSaveQuirks(org, project, linesToQuirks(quirksValue, existingQuirks.data ?? [])));
    },
    onSuccess: (_data, recipe) => {
      qc.invalidateQueries({ queryKey: ["autorun-recipe", org, project] });
      qc.invalidateQueries({ queryKey: ["autorun-quirks", org, project] });
      toast.success(recipe ? "Sign-in recipe saved." : "Quirks saved.");
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  const submit = () => {
    setProblem("");
    if (recipeEmpty) {
      // Nothing typed in the recipe box: this project may simply not have
      // one yet, and that is not a reason to block saving the quirks.
      save.mutate(null);
      return;
    }
    // A cast, not a runtime validation: the Rust side is the one place
    // that has to actually validate a recipe (`SignInRecipe::validate`),
    // and the save call below is what surfaces its verdict.
    let parsed: SignInRecipe_Deserialize;
    try {
      parsed = JSON.parse(value) as SignInRecipe_Deserialize;
    } catch (e) {
      setProblem(`That is not valid JSON: ${(e as Error).message}`);
      return;
    }
    save.mutate(parsed);
  };

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-3 overflow-y-auto p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Sign-in recipe</h2>
        <p className="mt-1 text-xs text-muted">
          How to sign in to this project's application, once, for every script. Use {"{{username}}"} and{" "}
          {"{{password}}"} where the account's login goes. Use {"{{password}}"} only on a real password
          field (type=password): the browser masks it there, and the run's pictures would show it
          anywhere else. "signed_in" is something only a signed-in page shows. "when_visible" handles a
          prompt that may or may not appear.
        </p>
      </div>
      {existing.isError && <p className="text-xs text-danger">{existing.error.message}</p>}
      <Textarea aria-label="Sign-in recipe JSON" className="min-h-[22rem] flex-1 font-mono text-xs"
        placeholder={PLACEHOLDER} value={value} onChange={(e) => setText(e.target.value)} />
      <div>
        <h3 className="text-sm font-semibold text-text">Known quirks</h3>
        <p className="mt-1 text-xs text-muted">
          One per line: something learned about this application that the next script - written by a
          person or an assistant - should not have to rediscover.
        </p>
      </div>
      {existingQuirks.isError && <p className="text-xs text-danger">{existingQuirks.error.message}</p>}
      <Textarea aria-label="Known quirks" className="min-h-[8rem] font-mono text-xs"
        value={quirksValue} onChange={(e) => setQuirksText(e.target.value)} />
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={blocked || save.isPending || (recipeEmpty && quirksEmpty)} onClick={submit}>
          <IconConfirm aria-hidden />
          {save.isPending ? "Saving" : recipeEmpty ? "Save quirks" : "Save recipe"}
        </Button>
      </div>
    </Modal>
  );
}

// The project's one sign-in recipe, as JSON. JSON on purpose, like the
// script editor: the format has to be proven by hand before anything
// friendlier is built on it.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "../../lib/toast";
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
  "after_sign_in": [
    { "kind": "when_visible", "selector": { "css": "#sidebar-toggle-menu:not(.active)" }, "within_ms": 1500,
      "then": [ { "kind": "click", "selector": { "css": "#sidebar-toggle-menu" } } ] }
  ],
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
  const [recipeProblem, setRecipeProblem] = useState("");
  const [quirksProblem, setQuirksProblem] = useState("");
  const loadedRecipe = existing.data ? JSON.stringify(existing.data, null, 2) : "";
  const loadedQuirks = (existingQuirks.data ?? []).map((q) => q.text).join("\n");
  const value = text ?? loadedRecipe;
  const quirksValue = quirksText ?? loadedQuirks;
  const recipeBlocked = existing.isLoading || existing.isError;
  const quirksBlocked = existingQuirks.isLoading || existingQuirks.isError;
  const recipeEmpty = value.trim() === "";
  const quirksEmpty = quirksValue.trim() === "";
  // Clearing every line out of a box that used to have quirks in it is a
  // real save (an empty list), not nothing to do.
  const quirksWereLoaded = (existingQuirks.data ?? []).length > 0;
  /** Typed into and not saved yet - what decides whether a save in the
   * OTHER section may close the dialog. */
  const recipeDirty = text !== null && text.trim() !== loadedRecipe.trim();
  const quirksDirty = quirksText !== null && quirksText !== loadedQuirks;

  // Two saves, one per section. Each writes only its own file: a recipe
  // the app refuses never costs the quirks typed below it, and saving a
  // quirk never re-sends a recipe nobody touched. A save closes the dialog
  // only when the other section has nothing unsaved in it - otherwise the
  // dialog stays open so that edit is not thrown away.
  const saveRecipe = useMutation({
    mutationFn: async (v: { recipe: SignInRecipe_Deserialize; close: boolean }) => {
      await unwrapStr(commands.autoRunSaveRecipe(org, project, v.recipe));
    },
    onSuccess: async (_data, v) => {
      toast.success("Sign-in recipe saved.");
      if (v.close) {
        qc.invalidateQueries({ queryKey: ["autorun-recipe", org, project] });
        onClose();
        return;
      }
      // Refetched before the box lets go of the typed text, so it never
      // flashes back to the old recipe in between.
      await qc.invalidateQueries({ queryKey: ["autorun-recipe", org, project] });
      setText(null);
    },
    onError: (e) => setRecipeProblem(e instanceof Error ? e.message : String(e)),
  });

  const saveQuirks = useMutation({
    mutationFn: async (_v: { close: boolean }) => {
      await unwrapStr(
        commands.autoRunSaveQuirks(org, project, linesToQuirks(quirksValue, existingQuirks.data ?? [])),
      );
    },
    onSuccess: async (_data, v) => {
      toast.success("Quirks saved.");
      if (v.close) {
        qc.invalidateQueries({ queryKey: ["autorun-quirks", org, project] });
        onClose();
        return;
      }
      await qc.invalidateQueries({ queryKey: ["autorun-quirks", org, project] });
      setQuirksText(null);
    },
    onError: (e) => setQuirksProblem(e instanceof Error ? e.message : String(e)),
  });

  const submitRecipe = () => {
    setRecipeProblem("");
    // A cast, not a runtime validation: the Rust side is the one place
    // that has to actually validate a recipe (`SignInRecipe::validate`),
    // and the save call below is what surfaces its verdict.
    let parsed: SignInRecipe_Deserialize;
    try {
      parsed = JSON.parse(value) as SignInRecipe_Deserialize;
    } catch (e) {
      setRecipeProblem(`That is not valid JSON: ${(e as Error).message}`);
      return;
    }
    saveRecipe.mutate({ recipe: parsed, close: !quirksDirty });
  };

  const submitQuirks = () => {
    setQuirksProblem("");
    saveQuirks.mutate({ close: !recipeDirty });
  };

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-4 overflow-y-auto p-5">
      <h2 className="text-sm font-semibold text-text">Sign-in recipe</h2>

      <section className="space-y-2">
        <p className="text-xs text-muted">
          How to sign in to this project's application, once, for every script. Use {"{{username}}"} and{" "}
          {"{{password}}"} where the account's login goes. Use {"{{password}}"} only on a real password
          field (type=password): the browser masks it there, and the run's pictures would show it
          anywhere else. "signed_in" is something only a signed-in page shows. "when_visible" handles a
          prompt that may or may not appear. "after_sign_in" runs after every sign-in, a saved session
          included, to leave the application the way scripts expect it - here, opening a menu that starts
          closed, only when it is closed.
        </p>
        {existing.isError && <p className="text-xs text-danger">{existing.error.message}</p>}
        <Textarea aria-label="Sign-in recipe JSON" className="min-h-[18rem] font-mono text-xs"
          placeholder={PLACEHOLDER} value={value} onChange={(e) => setText(e.target.value)} />
        {recipeProblem && <p className="text-xs text-danger">{recipeProblem}</p>}
        <div className="flex justify-end">
          <Button size="sm" disabled={recipeBlocked || saveRecipe.isPending || recipeEmpty} onClick={submitRecipe}>
            <IconConfirm aria-hidden />
            {saveRecipe.isPending ? "Saving" : "Save recipe"}
          </Button>
        </div>
      </section>

      <section className="space-y-2 border-t border-border pt-4">
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
        {quirksProblem && <p className="text-xs text-danger">{quirksProblem}</p>}
        <div className="flex justify-end">
          <Button
            size="sm"
            disabled={quirksBlocked || saveQuirks.isPending || (quirksEmpty && !quirksWereLoaded)}
            onClick={submitQuirks}
          >
            <IconConfirm aria-hidden />
            {saveQuirks.isPending ? "Saving" : "Save quirks"}
          </Button>
        </div>
      </section>

      <div className="flex justify-end border-t border-border pt-3">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}

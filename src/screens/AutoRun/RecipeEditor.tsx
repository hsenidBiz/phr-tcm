// The project's one sign-in recipe, as JSON. JSON on purpose, like the
// script editor: the format has to be proven by hand before anything
// friendlier is built on it.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type SignInRecipe_Deserialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import QuirksList from "./QuirksList";

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
  const [recipeProblem, setRecipeProblem] = useState("");
  /** A note typed into the quirks list and not saved yet - a recipe save
   * leaves the dialog open rather than throw it away. */
  const [quirksPending, setQuirksPending] = useState(false);
  const loadedRecipe = existing.data ? JSON.stringify(existing.data, null, 2) : "";
  const value = text ?? loadedRecipe;
  const recipeBlocked = existing.isLoading || existing.isError;
  const recipeEmpty = value.trim() === "";

  // The recipe has its own Save and writes only its own file; each change
  // to the quirks list below is saved the moment it is made, by its own
  // command. Saving the recipe closes the dialog unless a note is still
  // being typed below.
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
    saveRecipe.mutate({ recipe: parsed, close: !quirksPending });
  };

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-4 overflow-y-auto p-5">
      <h2 className="text-sm font-semibold text-text">Sign-in recipe</h2>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold text-text">Recipe</h3>
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
            Things learned about this application that the next script or API template - written by a
            person or an assistant - should not have to rediscover. A note an assistant filed with a repair
            shows what the runs since have said about it. Retire a note that no longer helps; it can be
            restored. Each change is saved as you make it.
          </p>
        </div>
        {existingQuirks.isError && <p className="text-xs text-danger">{existingQuirks.error.message}</p>}
        {existingQuirks.isSuccess && (
          <QuirksList org={org} project={project} quirks={existingQuirks.data} onPendingChange={setQuirksPending} />
        )}
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

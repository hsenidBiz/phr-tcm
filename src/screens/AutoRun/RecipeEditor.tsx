// The project's one sign-in recipe, as JSON. JSON on purpose, like the
// script editor: the format has to be proven by hand before anything
// friendlier is built on it.

import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type SignInRecipe_Deserialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { Term } from "../../components/Term";

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
  "close_when_shown": [
    { "name": "cookie banner", "shown": { "css": "#cookieBanner.show" }, "close": { "css": "#btnCookieClose" } }
  ],
  "wait_while_shown": [
    { "name": "page loader", "shown": { "css": "#ehrmLoader" } }
  ],
  "allowed_origins": [],
  "session_minutes": 480
}`;

export default function RecipeEditor({ org, project, onClose }: { org: string; project: string; onClose: () => void }) {
  // What it holds is this environment's: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const qc = useQueryClient();
  const existing = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: () => unwrapStr(commands.autoRunLoadRecipe(org, project)),
    retry: false,
  });
  const [text, setText] = useState<string | null>(null);
  const [recipeProblem, setRecipeProblem] = useState("");
  const loadedRecipe = existing.data ? JSON.stringify(existing.data, null, 2) : "";
  const value = text ?? loadedRecipe;
  const recipeBlocked = existing.isLoading || existing.isError;
  const recipeEmpty = value.trim() === "";

  // The recipe has its own Save and writes only its own file. Saving it
  // closes the dialog.
  const saveRecipe = useMutation({
    mutationFn: async (recipe: SignInRecipe_Deserialize) => {
      await unwrapStr(commands.autoRunSaveRecipe(org, project, recipe));
    },
    onSuccess: () => {
      toast.success("Sign-in recipe saved.");
      qc.invalidateQueries({ queryKey: ["autorun-recipe", org, project] });
      onClose();
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
    saveRecipe.mutate(parsed);
  };

  return (
    <Modal onClose={onClose} className={`${MODAL_LARGE} flex flex-col gap-4 overflow-y-auto p-5`}>
      <h2 className="text-sm font-semibold text-text">
        Sign-in <Term name="recipe">recipe</Term>
      </h2>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold text-text">Recipe</h3>
        <p className="text-xs text-muted">
          How to sign in to this project's application, once, for every script. Use {"{{username}}"} and{" "}
          {"{{password}}"} where the account's login goes. Use {"{{password}}"} only on a real password
          field (type=password): the browser masks it there, and the run's pictures would show it
          anywhere else. "signed_in" is something only a signed-in page shows. "when_visible" handles a
          prompt that may or may not appear. "after_sign_in" runs after every sign-in, a saved session
          included, to leave the application the way scripts expect it - here, opening a menu that starts
          closed, only when it is closed. "close_when_shown" lists pop-ups the runner closes by itself: right
          before each click, fill, drag or upload it looks once, without waiting, and clicks "close" while
          "shown" is up - so no script needs a guard for them. Point "close" at the pop-up's close button,
          never at one that accepts something. "wait_while_shown" lists loading indicators the runner waits
          out by itself: right before each click, fill, check or wait it looks once, and while "shown" is
          visible it waits for it to go - up to 90 s, or "max_ms" (1000 to 180000) - before the action's own
          timeout starts, so no script needs a long first-step wait.
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

      <div className="flex justify-end border-t border-border pt-3">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}

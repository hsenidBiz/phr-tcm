// The project's one sign-in recipe, as JSON. JSON on purpose, like the
// script editor: the format has to be proven by hand before anything
// friendlier is built on it.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type SignInRecipe_Deserialize } from "../../bindings";
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

export default function RecipeEditor({ org, project, onClose }: { org: string; project: string; onClose: () => void }) {
  const qc = useQueryClient();
  const existing = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: () => unwrapStr(commands.autoRunLoadRecipe(org, project)),
    retry: false,
  });
  const [text, setText] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const value = text ?? (existing.data ? JSON.stringify(existing.data, null, 2) : "");
  const blocked = existing.isLoading || existing.isError;

  const save = useMutation({
    mutationFn: (recipe: SignInRecipe_Deserialize) => unwrapStr(commands.autoRunSaveRecipe(org, project, recipe)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["autorun-recipe", org, project] });
      toast.success("Sign-in recipe saved.");
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  const submit = () => {
    setProblem("");
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
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-3 p-5">
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
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={blocked || save.isPending || value.trim() === ""} onClick={submit}>
          <IconConfirm aria-hidden />
          {save.isPending ? "Saving" : "Save recipe"}
        </Button>
      </div>
    </Modal>
  );
}

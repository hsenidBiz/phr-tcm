// The site a project's runs go to, edited on its own - without opening
// the whole sign-in recipe as JSON to change one address.
//
// There is no separate "site address" on disk: it is the recipe's
// `start_url` and `allowed_origins`. So a save reads the saved recipe back
// fresh, changes just those two fields and writes the whole recipe through
// the same command the recipe editor uses (`auto_run_save_recipe`), which
// runs `SignInRecipe::validate` - a bad address is refused there, in the
// app's own words, and nothing is written. Every other field goes back
// as it was read: the Rust suite's
// `a_recipe_parses_with_defaults_and_round_trips` serializes a recipe with
// every optional field set (after_sign_in, allowed_origins,
// session_minutes) and checks it deserializes to the same recipe and
// matches the JSON it came from.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type SignInRecipe_Deserialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input, Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

/** "hr.example.internal" from "https://hr.example.internal/login"; the text
 * itself when it is not an address at all. */
export function siteHost(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

/** The box's lines, trimmed, blanks dropped - one origin each. */
function lines(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}

export default function SiteAddressDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const recipeKey = ["autorun-recipe", org, project];
  // The same query the recipe editor and the Setup card use, so the boxes
  // open already filled when the screen has it.
  const existing = useQuery({
    queryKey: recipeKey,
    queryFn: async () => (await unwrapStr(commands.autoRunLoadRecipe(org, project))) ?? null,
    retry: false,
  });
  const [start, setStart] = useState<string | null>(null);
  const [also, setAlso] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const startValue = start ?? existing.data?.start_url ?? "";
  const alsoValue = also ?? (existing.data?.allowed_origins ?? []).join("\n");

  const save = useMutation({
    mutationFn: async () => {
      // Read back fresh rather than trusting what this dialog opened with,
      // so the fields it does not touch are the ones on disk right now.
      const current = await unwrapStr(commands.autoRunLoadRecipe(org, project));
      if (!current) {
        throw new Error("this project has no sign-in recipe yet - set one up in Sign-in first");
      }
      // Serialize and Deserialize are the same JSON for a recipe (the Rust
      // round-trip test proves it); the generated types only differ in
      // how optional fields are spelled, hence the cast.
      const next = {
        ...current,
        start_url: startValue.trim(),
        allowed_origins: lines(alsoValue),
      } as unknown as SignInRecipe_Deserialize;
      await unwrapStr(commands.autoRunSaveRecipe(org, project, next));
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: recipeKey });
      toast.success("Site address saved.");
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  const noRecipe = existing.isSuccess && existing.data == null;
  const blocked = existing.isLoading || existing.isError || noRecipe;

  return (
    <Modal onClose={onClose} className="flex w-full max-w-lg flex-col gap-3 p-5">
      <h2 className="text-sm font-semibold text-text">Site address</h2>
      {existing.isError && <p className="text-xs text-danger">{existing.error.message}</p>}
      {noRecipe && (
        <p className="text-xs text-muted">
          This project has no sign-in recipe yet - set one up in Sign-in first.
        </p>
      )}
      <label className="space-y-1">
        <span className="text-xs font-medium text-muted">Start address</span>
        <Input
          aria-label="Start address"
          placeholder="https://hr.example.internal/"
          value={startValue}
          disabled={blocked}
          onChange={(e) => setStart(e.target.value)}
        />
      </label>
      <label className="space-y-1">
        <span className="text-xs font-medium text-muted">Also allowed</span>
        <Textarea
          aria-label="Also allowed"
          className="min-h-[5rem] font-mono text-xs"
          placeholder="https://login.example.com"
          value={alsoValue}
          disabled={blocked}
          onChange={(e) => setAlso(e.target.value)}
        />
        <span className="block text-xs text-faint">
          Other sites scripts may open, one per line.
        </span>
      </label>
      {/* Saved sessions are kept per account, not per address
          (`autorun/sessions.rs`), so a new address does not by itself
          throw one away: the next sign-in tries it there first and signs
          in afresh only when that site does not accept it
          (`autorun/signin.rs`). */}
      <p className="text-xs text-muted">
        Runs sign in here and start from here. A saved sign-in is tried first, and the run signs in
        afresh if this site does not accept it.
      </p>
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          disabled={blocked || save.isPending || startValue.trim() === ""}
          onClick={() => {
            setProblem("");
            save.mutate();
          }}
        >
          <IconConfirm aria-hidden />
          {save.isPending ? "Saving" : "Save"}
        </Button>
      </div>
    </Modal>
  );
}

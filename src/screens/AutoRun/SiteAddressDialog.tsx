// The site the active environment's runs go to, edited on its own -
// without opening the whole sign-in recipe as JSON to change one address.
//
// The address belongs to the ACTIVE environment (its start address and
// allowed sites), saved with `env_save`; the sign-in recipe file is never
// written from here. An environment with no address of its own uses the
// recipe's, which the empty box says in words and shows as its
// placeholder. Rust validates the address on save and refuses a bad one in
// the app's own words, and nothing is written.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input, Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { activeEnvironment, envKeys, loadEnvironments, toInput, useEnvironments } from "../../lib/environments";
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
  // The recipe is read only for the address an empty box falls back to.
  const recipe = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadRecipe(org, project))) ?? null,
    retry: false,
  });
  const envs = useEnvironments();
  const env = activeEnvironment(envs.data);
  const [start, setStart] = useState<string | null>(null);
  const [also, setAlso] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const startValue = start ?? env?.start_url ?? "";
  const alsoValue = also ?? (env?.allowed_origins ?? []).join("\n");

  const save = useMutation({
    mutationFn: async () => {
      // Read back fresh rather than trusting what this dialog opened with,
      // so the fields it does not touch are the ones saved right now.
      const current = activeEnvironment(await loadEnvironments());
      if (!current) throw new Error("there is no active environment to save the address to");
      const res = await commands.envSave({
        ...toInput(current),
        start_url: startValue.trim(),
        allowed_origins: lines(alsoValue),
      });
      if (res.status === "error") throw new Error(res.error);
      return res.data;
    },
    onSuccess: (view) => {
      qc.setQueryData(envKeys.list, view);
      toast.success("Site address saved.");
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  const blocked = envs.isLoading || envs.isError || !env;
  const recipeAddress = recipe.data?.start_url ?? "";

  return (
    <Modal onClose={onClose} className="flex w-full max-w-lg flex-col gap-3 p-5">
      <h2 className="text-sm font-semibold text-text">Site address</h2>
      {envs.isError && <p className="text-xs text-danger">{envs.error.message}</p>}
      {env && (
        <p className="text-xs text-muted">
          For the <span className="font-medium text-text">{env.name}</span> environment.
        </p>
      )}
      <label className="space-y-1">
        <span className="text-xs font-medium text-muted">Start address</span>
        <Input
          aria-label="Start address"
          placeholder={recipeAddress || "https://hr.example.internal/"}
          value={startValue}
          disabled={blocked}
          onChange={(e) => setStart(e.target.value)}
        />
        {startValue.trim() === "" && !blocked && recipeAddress !== "" && (
          <span className="block text-xs text-faint">Using the sign-in recipe's address</span>
        )}
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
          disabled={blocked || save.isPending}
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

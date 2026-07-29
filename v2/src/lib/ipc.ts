import type { AdoError } from "../bindings";

export function describeAdoError(e: AdoError): string {
  switch (e.kind) {
    case "Unauthorized":
      return "Not authorized - sign in again.";
    case "RateLimited":
      return `Rate limited - retry in ${e.detail.retry_after_secs}s.`;
    case "Forbidden":
      return "You don't have permission for this resource.";
    case "NotFound":
      return "Not found.";
    case "Http": {
      // The body usually carries ADO's real explanation ("Rule Error for
      // field Remaining Work...") - surface it instead of a bare status,
      // so callers (and users) see WHICH rule failed.
      try {
        const m = JSON.parse(e.detail.body)?.message;
        if (typeof m === "string" && m.trim()) return m;
      } catch {
        // Not JSON. That is the normal shape for the errors this APP
        // raises about Azure DevOps rather than receives from it - status
        // 0 with a written explanation ("Run #12 was created but those
        // outcomes were NOT recorded..."). Every one of those was being
        // shown as "Azure DevOps returned HTTP 0.", so the sentence
        // written to tell the user what to do never reached them.
        const body = e.detail.body?.trim() ?? "";
        // A server error page is not an explanation - only prose, and only
        // as much of it as belongs in a toast.
        if (body && !body.startsWith("<") && body.length <= 400) return body;
      }
      return `Azure DevOps returned HTTP ${e.detail.status}.`;
    }
    case "Network":
      return `Network error: ${e.detail}`;
  }
}

type IpcResult<T> = { status: "ok"; data: T } | { status: "error"; error: AdoError };

/** Unwrap a specta Result<T, AdoError> command; throws a readable Error. */
export async function unwrap<T>(p: Promise<IpcResult<T>>): Promise<T> {
  const r = await p;
  if (r.status === "error") throw new Error(describeAdoError(r.error));
  return r.data;
}

type StringResult<T> = { status: "ok"; data: T } | { status: "error"; error: string };

/** Unwrap a specta Result<T, String> command (plain-string errors). */
export async function unwrapStr<T>(p: Promise<StringResult<T>>): Promise<T> {
  const r = await p;
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}

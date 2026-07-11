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
    case "Http":
      return `Azure DevOps returned HTTP ${e.detail.status}.`;
    case "Network":
      return `Network error: ${e.detail}`;
  }
}

type IpcResult<T> = { status: "ok"; data: T } | { status: "error"; error: AdoError };

/** Unwrap a specta Result command; throws a readable Error for TanStack Query. */
export async function unwrap<T>(p: Promise<IpcResult<T>>): Promise<T> {
  const r = await p;
  if (r.status === "error") throw new Error(describeAdoError(r.error));
  return r.data;
}

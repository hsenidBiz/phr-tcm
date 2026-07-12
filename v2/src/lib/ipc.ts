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

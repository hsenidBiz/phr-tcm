// What the assistants left for a person to decide: proposed accounts, a
// proposed sign-in, divergences waiting for a ruling. One query shared by
// the sidebar's count, the bell's notices and the Waiting for you list.
//
// The key carries the active environment, so switching environments never
// shows the previous one's items while the new ones load. Rust says when
// any of it changes (`useAutoRunDataSync` reads this key again).

import { useQuery } from "@tanstack/react-query";
import { commands, type WaitingItems } from "../bindings";
import { useEnvironments } from "./environments";
import { unwrapStr } from "./ipc";

export const WAITING_KEY = ["autorun-waiting"] as const;

export function useWaitingItems(org: string, project: string, enabled: boolean): WaitingItems | undefined {
  const envId = useEnvironments(enabled).data?.active ?? "";
  return useQuery({
    queryKey: [...WAITING_KEY, org, project, envId],
    queryFn: () => unwrapStr(commands.autoRunWaitingItems(org, project)),
    enabled: enabled && Boolean(org && project),
    retry: false,
  }).data;
}

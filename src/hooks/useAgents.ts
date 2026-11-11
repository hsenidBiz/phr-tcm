// The assistants connected now and the sign-in trial, kept current: Rust
// sends the whole picture whenever any of it changes (`autorun-agents-changed`,
// at most once a second). Listen first and load after, so a change between
// the two is not missed; an event that lands during the load beats it.

import { useEffect, useState } from "react";
import { commands, events, type AgentView, type SignInTrial } from "../bindings";

export function useAgents(): { agents: AgentView[]; trial: SignInTrial | null } {
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [trial, setTrial] = useState<SignInTrial | null>(null);

  useEffect(() => {
    let live = true;
    let heard = false;
    const un = events.autorunAgentsChanged.listen((e) => {
      heard = true;
      if (!live) return;
      setAgents(e.payload.agents);
      setTrial(e.payload.trial ?? null);
    });
    void un
      .catch(() => {})
      .then(async () => {
        if (!live) return;
        const list = await commands.autoRunAgents();
        if (live && !heard && Array.isArray(list)) setAgents(list);
        const t = await commands.autoRunSignInTrial();
        if (live && !heard) setTrial(t ?? null);
      })
      .catch(() => {});
    return () => {
      live = false;
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  return { agents, trial };
}

// The Work Manager's "New Work Item" screen: the full creation form that
// replaced the board's cramped quick-create row. Everything is set before
// the item is created - type, title, assignee, area, iteration, priority,
// tags, description, and an optional parent PBI (Hierarchy-Reverse link,
// so the item nests under it on boards and backlogs).

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type CreatedItem, type PbiHit } from "../bindings";
import PbiPicker from "../components/PbiPicker";
import TagsField from "../components/TagsField";
import { Button } from "../components/ui/button";
import Combobox from "../components/ui/combobox";
import { Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { unwrap } from "../lib/ipc";
import { cached } from "../lib/localCache";
import { iterationDetails } from "../lib/iterations";

const TYPES = ["Task", "Bug", "Product Backlog Item"];

export default function CreateWorkItem({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [wiType, setWiType] = useState("Task");
  const [title, setTitle] = useState("");
  const [assignee, setAssignee] = useState("");
  const [area, setArea] = useState("");
  const [iteration, setIteration] = useState("");
  const [priority, setPriority] = useState("");
  const [tags, setTags] = useState("");
  const [description, setDescription] = useState("");
  const [parent, setParent] = useState<PbiHit | null>(null);
  const [created, setCreated] = useState<CreatedItem | null>(null);

  const members = useQuery({
    // Same key + cache as the drawer: one members fetch serves both.
    queryKey: ["members", org, project],
    queryFn: () =>
      cached(`members:${org}/${project}`, 24 * 60 * 60_000, () =>
        unwrap(commands.listTeamMembers(org, project)),
      ),
    enabled: Boolean(org && project),
    staleTime: 24 * 60 * 60_000,
    retry: false,
  });
  const areas = useQuery({
    queryKey: ["classification", org, project, "areas"],
    queryFn: () => unwrap(commands.classificationPaths(org, project, "areas")),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
  });
  const iterations = useQuery({
    queryKey: ["iterations-dated", org, project],
    queryFn: () => unwrap(commands.listIterations(org, project)),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
  });

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        commands.createWorkItem(org, project, {
          wi_type: wiType,
          title: title.trim(),
          assigned_to: assignee || null,
          area_path: area || null,
          iteration_path: iteration || null,
          tags: tags.trim() || null,
          priority: priority ? Number(priority) : null,
          description: description.trim() || null,
          parent_id: parent?.id ?? null,
        }),
      ),
    onSuccess: (item) => {
      setCreated(item);
      toast.success(`Created ${wiType} #${item.id}`);
      // Any open board should show the new item on next visit.
      qc.invalidateQueries({ queryKey: ["board"] });
    },
    onError: (e) => toast.error(`Create failed: ${e.message}`),
  });

  const resetForKeep = () => {
    // "Create another" keeps the contextual choices (type, area, iteration,
    // parent) and clears only the per-item content.
    setTitle("");
    setTags("");
    setDescription("");
    setCreated(null);
  };

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">Pick an organization and project in the bar above first.</p>
    );
  }

  if (created) {
    return (
      <div className="max-w-xl space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">
          Created {wiType} <span className="id-mono text-accent">#{created.id}</span>
        </h2>
        <p className="text-xs text-muted">
          {parent ? `Linked under PBI #${parent.id}.` : "Not linked to a parent."}
        </p>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => openUrl(created.url).catch(() => toast.error("Could not open the browser."))}
          >
            Open in Azure DevOps
          </Button>
          <Button size="sm" onClick={resetForKeep}>
            Create another
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-xl space-y-4">
      <div className="flex gap-2">
        <label className="block text-xs text-muted">
          <span className="mb-1 block">Type</span>
          <Select
            aria-label="Work item type"
            className="w-48"
            value={wiType}
            onChange={(e) => setWiType(e.target.value)}
          >
            {TYPES.map((t) => (
              <option key={t}>{t}</option>
            ))}
          </Select>
        </label>
        <label className="block flex-1 text-xs text-muted">
          <span className="mb-1 block">Title</span>
          <Input
            aria-label="Title"
            className="w-full"
            placeholder="What needs doing?"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
        </label>
      </div>

      <div className="flex gap-2">
        <label className="block flex-1 text-xs text-muted">
          <span className="mb-1 block">Assign to</span>
          <Select
            aria-label="Assign to"
            className="w-full"
            value={assignee}
            onChange={(e) => setAssignee(e.target.value)}
          >
            <option value="">Unassigned</option>
            {(members.data ?? []).map((m) => (
              <option key={m.unique_name} value={m.unique_name}>
                {m.display_name}
              </option>
            ))}
          </Select>
        </label>
        <label className="block w-32 text-xs text-muted">
          <span className="mb-1 block">Priority</span>
          <Select
            aria-label="Priority"
            className="w-full"
            value={priority}
            onChange={(e) => setPriority(e.target.value)}
          >
            <option value="">Default</option>
            {[1, 2, 3, 4].map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </Select>
        </label>
      </div>

      <div className="flex gap-2">
        <label className="block flex-1 text-xs text-muted">
          <span className="mb-1 block">Area</span>
          <Combobox
            ariaLabel="Area"
            className="w-full"
            placeholder="Same as project root"
            value={area}
            options={areas.data ?? []}
            onChange={setArea}
          />
        </label>
        <label className="block flex-1 text-xs text-muted">
          <span className="mb-1 block">Iteration</span>
          <Combobox
            ariaLabel="Iteration"
            className="w-full"
            placeholder="Backlog (none)"
            value={iteration}
            options={(iterations.data ?? []).map((i) => i.path)}
            details={iterationDetails(iterations.data ?? [])}
            onChange={setIteration}
          />
        </label>
      </div>

      <label className="block text-xs text-muted">
        <span className="mb-1 block">Tags</span>
        <TagsField org={org} project={project} className="w-full" value={tags} onChange={setTags} />
      </label>

      <label className="block text-xs text-muted">
        <span className="mb-1 block">Description</span>
        <Textarea
          aria-label="Description"
          className="h-28 w-full"
          placeholder="Context, acceptance criteria, links…"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </label>

      <div className="text-xs text-muted">
        Parent PBI (optional - nests the item under it on the board)
        <div className="mt-1 max-w-md">
          <PbiPicker org={org} project={project} pbi={parent} onChange={setParent} />
        </div>
      </div>

      <div className="pt-1">
        <Button
          disabled={!title.trim() || create.isPending}
          onClick={() => create.mutate()}
        >
          {create.isPending ? "Creating…" : `Create ${wiType}`}
        </Button>
      </div>
    </div>
  );
}

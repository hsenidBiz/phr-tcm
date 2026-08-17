import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { unwrap } from "../lib/ipc";
import TagField from "./ui/tagfield";

/** Project-aware tag input: fetches the project's existing tag names as
 * suggestions and renders the searchable multi-select chip field. Value in
 * and out is the semicolon-joined string the TestCase model stores. */
export default function TagsField({
  org,
  project,
  value,
  onChange,
  placeholder,
  ariaLabel,
  className,
}: {
  org: string;
  project: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  /** Override the default "Tags" name when two instances share a screen. */
  ariaLabel?: string;
  className?: string;
}) {
  const tags = useQuery({
    queryKey: ["project-tags", org, project],
    queryFn: () => unwrap(commands.listProjectTags(org, project)),
    enabled: Boolean(org && project),
    staleTime: 10 * 60_000,
  });

  return (
    <TagField
      value={value}
      onChange={onChange}
      suggestions={tags.data ?? []}
      placeholder={placeholder}
      ariaLabel={ariaLabel}
      className={className}
    />
  );
}

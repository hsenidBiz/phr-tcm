import { useState } from "react";
import { IconCancel, IconConfirm } from "../lib/actionIcons";
import { loadDefaultTags, saveDefaultTags } from "../lib/defaultTags";
import TagsField from "./TagsField";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";

/** The one place a project's default tags are set.
 *
 * They used to live in Settings, two screens away from the only screen
 * that uses them, which made a setting about writing test cases something
 * you had to already know existed. Opening it from Manual Entry puts it
 * beside its effect: the set being edited here is the set visible on the
 * form behind the dialog.
 *
 * Saving an empty field clears the defaults - that is what the storage
 * helper does with an empty value, and it is the only way back to a form
 * with no fixed tags on it.
 */
export default function DefaultTagsDialog({
  org,
  project,
  onClose,
  onSaved,
}: {
  org: string;
  project: string;
  onClose: () => void;
  /** The saved set, so the form behind can fix it without a reload. */
  onSaved: (tags: string) => void;
}) {
  const [tags, setTags] = useState(() => loadDefaultTags(org, project));

  return (
    <Modal onClose={onClose} className="w-full max-w-md space-y-4 p-4">
      <div className="space-y-1">
        <h2 className="text-sm font-semibold text-text">Default tags</h2>
        <p className="text-sm text-muted">
          Added to every new test case you write for this project, and held on the
          form so one cannot go up without them. You can still add more tags to any
          single case. Clearing this leaves the form free again.
        </p>
      </div>

      <TagsField
        org={org}
        project={project}
        ariaLabel="Default tags"
        className="w-full"
        value={tags}
        onChange={setTags}
        placeholder="Tags every case should carry…"
      />

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          onClick={() => {
            saveDefaultTags(org, project, tags);
            // What was STORED, not what was typed - the helper trims, and
            // the form has to fix exactly what a reload would give it.
            onSaved(loadDefaultTags(org, project));
            onClose();
          }}
        >
          <IconConfirm aria-hidden />
          Save
        </Button>
      </div>
    </Modal>
  );
}

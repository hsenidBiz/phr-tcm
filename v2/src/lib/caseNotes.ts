/** Personal per-test-case comments for the View Test Cases tab. Saved to
 * localStorage only (never written to Azure DevOps) - a scratchpad for
 * "this case needs updating" notes. Keyed per org since work item ids are
 * org-scoped. */

const key = (org: string) => `tcm-v2-case-notes:${org}`;

export function loadNotes(org: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(key(org));
    return raw ? (JSON.parse(raw) as Record<string, string>) : {};
  } catch {
    return {};
  }
}

/** Save one case's note; an empty/whitespace note removes the entry. */
export function saveNote(org: string, caseId: number, text: string): Record<string, string> {
  const notes = loadNotes(org);
  const trimmed = text.trim();
  if (trimmed) notes[String(caseId)] = trimmed;
  else delete notes[String(caseId)];
  try {
    localStorage.setItem(key(org), JSON.stringify(notes));
  } catch {
    // storage unavailable -> session-only
  }
  return notes;
}

/**
 * The widths of Auto Run's dialogs, in one place so they stay consistent.
 *
 * Large: the dialogs that show data (scripts, accounts, areas, a run). About
 * 94% of the window, up to 1400px, and no taller than 90% of it.
 * Short form: a few fields and a button.
 * Confirm: a question and its answer; this one stays narrow on purpose.
 */
export const MODAL_LARGE = "w-[min(94vw,1400px)] max-w-none max-h-[90vh]";
export const MODAL_SHORT = "w-full max-w-2xl";
export const MODAL_CONFIRM = "w-full max-w-md";

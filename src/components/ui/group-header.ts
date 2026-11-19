/** The row a group's header sits in on every grouped list (Auto Test, View,
 * Update, Run Tests, API templates, the queue). The gap above a header
 * equals the gap below it: the groups sit in a column with an 8px gap
 * (`GROUP_COLUMN`) and each group's header and cards in a 4px one
 * (`GROUP_BODY`), and the row itself has 4px under it and none above. */
export const GROUP_HEADER_ROW = "flex w-full items-center gap-3 pb-1";
/** The column the groups sit in: 8px between one group and the next. */
export const GROUP_COLUMN = "space-y-2";
/** One group: its header, then its cards, 4px apart. */
export const GROUP_BODY = "space-y-1";

// A pull request thread's status, open or settled. Its own module rather
// than living in components/PrThreads: the notification bell's hook needs
// only this, and importing it from the component pulled the thread view and
// the whole design system into the startup bundle.

/** Azure DevOps has more statuses than it has meanings. Everything that is
 * not still open counts as settled, including the empty string it sends for
 * a thread nobody has ever resolved either way - that one reads as active,
 * which is why the check is written as "not one of the settled ones"
 * rather than as a list of open ones. */
const SETTLED = ["fixed", "wontfix", "closed", "bydesign"];
export function isResolved(status: string): boolean {
  return SETTLED.includes(status.trim().toLowerCase());
}

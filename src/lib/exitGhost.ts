/**
 * A close animation for something React is about to remove.
 *
 * React takes a dialog out of the page the moment the screen stops
 * rendering it, and twenty-odd screens do exactly that with their own
 * state. Rather than teach every one of them to keep the dialog mounted
 * while it fades, the dialog leaves a COPY of itself behind on the way out:
 * a plain DOM clone with the closing class, which plays the exit animation
 * and removes itself. The copy is a picture, not a control - hidden from
 * assistive tech, inert, unclickable, stripped of ids - so nothing can find
 * it or land on it during the fraction of a second it is there.
 *
 * Under prefers-reduced-motion nothing is left behind at all.
 */

const reducedMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

/**
 * Clone `node` into the page with `.is-closing` and remove the clone after
 * `ms`. Call it while `node` is still in the document - its scroll
 * positions are read then - and let React remove the original as usual.
 * Returns a cancel that removes the copy at once.
 */
export function leaveExitGhost(node: HTMLElement, ms: number): () => void {
  if (reducedMotion()) return () => {};

  const ghost = node.cloneNode(true) as HTMLElement;
  // Scrolled content stays scrolled: a clone starts at the top, and a long
  // dialog would visibly jump as it fades.
  const from = [node, ...node.querySelectorAll<HTMLElement>("*")];
  const to = [ghost, ...ghost.querySelectorAll<HTMLElement>("*")];
  from.forEach((el, i) => {
    if (el.scrollTop || el.scrollLeft) {
      to[i].scrollTop = el.scrollTop;
      to[i].scrollLeft = el.scrollLeft;
    }
  });
  // A picture carries nothing that names or finds a control: no ids,
  // roles, labels or test hooks. It is aria-hidden as a whole, but a label
  // query does not honour that, and a test that has just closed a dialog
  // must not find its copy.
  for (const el of to) {
    for (const attr of [...el.attributes]) {
      const n = attr.name;
      if (n === "id" || n === "role" || n === "for" || n === "name" || n === "placeholder" || n.startsWith("aria-") || n.startsWith("data-testid")) {
        el.removeAttribute(n);
      }
    }
  }

  ghost.classList.add("is-closing");
  ghost.setAttribute("data-exit-ghost", "");
  ghost.setAttribute("aria-hidden", "true");
  ghost.setAttribute("inert", "");
  ghost.style.pointerEvents = "none";
  document.body.appendChild(ghost);

  let done = false;
  const remove = () => {
    if (done) return;
    done = true;
    window.clearTimeout(timer);
    ghost.remove();
  };
  const timer = window.setTimeout(remove, ms);
  return remove;
}

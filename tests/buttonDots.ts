/**
 * Finds buttons whose visible label or aria-label ends in an ellipsis
 * ("…" or "..."). Shared by the ui-consistency gate and its own test.
 *
 * A button is a <Button>, <button> or menu-item component, or any element
 * with role="button" or role="menuitem". Every string it can show counts,
 * however deep: text, string literals, template tails, both sides of a
 * ternary, `&&`, `||` and `??`, children of nested elements, and a label
 * held in a same-file `const` the button renders. Status text such as
 * "Loading…" in a <p> or <span> outside a button, and select placeholders,
 * are not buttons and keep their dots.
 */
import ts from "typescript";

const BUTTON_TAGS = new Set(["Button", "button", "MenuItem", "DropdownMenuItem"]);
const BUTTON_ROLES = new Set(["button", "menuitem"]);
const ENDS_IN_DOTS = /(…|\.\.\.)\s*$/;

/** Every string `node` could put on screen, following same-file consts. */
function shownStrings(node: ts.Node, consts: Map<string, ts.Expression>, out: string[], seen = new Set<string>()): void {
  if (ts.isJsxText(node)) {
    if (node.text.trim() !== "") out.push(node.text);
    return;
  }
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    out.push(node.text);
    return;
  }
  if (ts.isTemplateExpression(node)) {
    const spans = node.templateSpans;
    out.push(spans.length > 0 ? spans[spans.length - 1].literal.text : node.head.text);
    for (const s of spans) shownStrings(s.expression, consts, out, seen);
    return;
  }
  if (ts.isIdentifier(node)) {
    const init = consts.get(node.text);
    if (init && !seen.has(node.text)) shownStrings(init, consts, out, new Set(seen).add(node.text));
    return;
  }
  // Attribute values are not shown text (className, onClick...), except
  // through the aria-label check below.
  if (ts.isJsxAttribute(node)) return;
  ts.forEachChild(node, (c) => shownStrings(c, consts, out, seen));
}

function constsOf(sf: ts.SourceFile): Map<string, ts.Expression> {
  const map = new Map<string, ts.Expression>();
  const visit = (n: ts.Node) => {
    if (ts.isVariableDeclaration(n) && ts.isIdentifier(n.name) && n.initializer) map.set(n.name.text, n.initializer);
    ts.forEachChild(n, visit);
  };
  visit(sf);
  return map;
}

export function buttonDots(source: string, fileName = "sample.tsx"): { line: number; label: string }[] {
  const sf = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const consts = constsOf(sf);
  const hits: { line: number; label: string }[] = [];
  const at = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1;

  const isButton = (opening: ts.JsxOpeningLikeElement) => {
    if (BUTTON_TAGS.has(opening.tagName.getText(sf))) return true;
    return opening.attributes.properties.some((a) => {
      if (!ts.isJsxAttribute(a) || a.name.getText(sf) !== "role" || !a.initializer) return false;
      const found: string[] = [];
      shownStrings(a.initializer, consts, found);
      return found.some((s) => BUTTON_ROLES.has(s));
    });
  };

  const check = (opening: ts.JsxOpeningLikeElement, children: readonly ts.Node[]) => {
    if (!isButton(opening)) return;
    for (const attr of opening.attributes.properties) {
      if (!ts.isJsxAttribute(attr) || attr.name.getText(sf) !== "aria-label" || !attr.initializer) continue;
      const found: string[] = [];
      shownStrings(attr.initializer, consts, found);
      for (const s of found) if (ENDS_IN_DOTS.test(s)) hits.push({ line: at(attr), label: s.trim() });
    }
    const found: string[] = [];
    for (const c of children) shownStrings(c, consts, found);
    for (const s of found) if (ENDS_IN_DOTS.test(s)) hits.push({ line: at(opening), label: s.trim() });
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node)) check(node.openingElement, node.children);
    else if (ts.isJsxSelfClosingElement(node)) check(node, []);
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return hits;
}

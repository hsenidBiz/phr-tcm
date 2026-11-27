/**
 * Finds buttons whose visible label or aria-label ends in an ellipsis
 * ("…" or "..."). Shared by the ui-consistency gate and its own test.
 *
 * Only a button counts: the JSX text of a <Button> or <button> (and of
 * the menu-item components), plus an aria-label on one. Status text such
 * as "Loading…" in a <p> or <span>, and select placeholders, are not
 * buttons and keep their dots.
 */
import ts from "typescript";

const BUTTON_TAGS = new Set(["Button", "button", "MenuItem", "DropdownMenuItem"]);
const ENDS_IN_DOTS = /(…|\.\.\.)\s*$/;

/** Every string a node could show: JSX text, string literals, template heads, both branches of a ternary. */
function shownStrings(node: ts.Node, out: string[]): void {
  if (ts.isJsxText(node)) {
    out.push(node.text);
    return;
  }
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    out.push(node.text);
    return;
  }
  if (ts.isTemplateExpression(node)) {
    // The last span is what ends the label.
    const spans = node.templateSpans;
    out.push(spans.length > 0 ? spans[spans.length - 1].literal.text : node.head.text);
    return;
  }
  if (ts.isJsxExpression(node) && node.expression) {
    shownStrings(node.expression, out);
    return;
  }
  if (ts.isConditionalExpression(node)) {
    shownStrings(node.whenTrue, out);
    shownStrings(node.whenFalse, out);
    return;
  }
  if (ts.isParenthesizedExpression(node)) {
    shownStrings(node.expression, out);
    return;
  }
  if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.BarBarToken) {
    shownStrings(node.left, out);
    shownStrings(node.right, out);
  }
}

/** A button label is the last text child; a trailing icon element does not hide the dots. */
function lastTextEndsInDots(children: readonly ts.Node[]): string | null {
  let last: string | null = null;
  for (const child of children) {
    const found: string[] = [];
    shownStrings(child, found);
    for (const s of found) {
      // Whitespace-only text between elements is not a label.
      if (s.trim() === "") continue;
      if (ENDS_IN_DOTS.test(s)) return s.trim();
      last = s.trim();
    }
  }
  return last !== null && ENDS_IN_DOTS.test(last) ? last : null;
}

export function buttonDots(source: string, fileName = "sample.tsx"): { line: number; label: string }[] {
  const sf = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const hits: { line: number; label: string }[] = [];
  const at = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1;

  const check = (opening: ts.JsxOpeningLikeElement, children: readonly ts.Node[]) => {
    if (!BUTTON_TAGS.has(opening.tagName.getText(sf))) return;
    for (const attr of opening.attributes.properties) {
      if (!ts.isJsxAttribute(attr) || attr.name.getText(sf) !== "aria-label" || !attr.initializer) continue;
      const found: string[] = [];
      shownStrings(attr.initializer, found);
      for (const s of found) if (ENDS_IN_DOTS.test(s)) hits.push({ line: at(attr), label: s.trim() });
    }
    const text = lastTextEndsInDots(children);
    if (text !== null) hits.push({ line: at(opening), label: text });
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node)) check(node.openingElement, node.children);
    else if (ts.isJsxSelfClosingElement(node)) check(node, []);
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return hits;
}

import type {
  ArrowFunctionExpression,
  FunctionDeclaration,
  FunctionExpression,
  JSXAttribute,
  JSXOpeningElement,
  Node,
  NodeOfType,
  NodeType,
  ParenthesizedExpression,
  TSAsExpression,
  TSNonNullExpression,
  TSSatisfiesExpression,
} from "@yuku-toolchain/types";

export type FunctionExpr = ArrowFunctionExpression | FunctionExpression;
export type FunctionNode = FunctionDeclaration | FunctionExpr;
type WrapperNode =
  | ParenthesizedExpression
  | TSNonNullExpression
  | TSAsExpression
  | TSSatisfiesExpression;

/**
 * Null-safe discriminant check that also narrows, so callers never cast. `K` is
 * a real {@link NodeType}, so a misspelled type is a compile error.
 */
export function is<K extends NodeType>(node: Node | null | undefined, type: K): node is NodeOfType<K> {
  return node?.type === type;
}

export function isFunctionExpr(node: Node): node is FunctionExpr {
  return is(node, "ArrowFunctionExpression") || is(node, "FunctionExpression");
}

export function isFunctionNode(node: Node): node is FunctionNode {
  return is(node, "FunctionDeclaration") || isFunctionExpr(node);
}

export function isWrapper(node: Node): node is WrapperNode {
  return (
    is(node, "ParenthesizedExpression") ||
    is(node, "TSNonNullExpression") ||
    is(node, "TSAsExpression") ||
    is(node, "TSSatisfiesExpression")
  );
}

/** Strip parens and TS cast wrappers down to the inner expression. */
export function unwrap(node: Node | null | undefined): Node | null {
  let current = node ?? null;
  while (current && isWrapper(current)) {
    current = current.expression;
  }
  return current;
}

/** Attribute name for a plain `name={...}` JSX attribute, else null. */
export function attrName(attr: JSXAttribute): string | null {
  return is(attr.name, "JSXIdentifier") ? attr.name.name : null;
}

/** The unwrapped expression of an `attr={expr}` value, else null. */
export function attrValue(attr: JSXAttribute): Node | null {
  if (!is(attr.value, "JSXExpressionContainer")) return null;
  const expr = unwrap(attr.value.expression);
  return expr && !is(expr, "JSXEmptyExpression") ? expr : null;
}

/** Tag name of a plain `<name ...>` element, else null. */
export function tagName(opening: JSXOpeningElement): string | null {
  return is(opening.name, "JSXIdentifier") ? opening.name.name : null;
}

export function isHostTag(name: string): boolean {
  return /^[a-z]/.test(name);
}

export function isEventAttr(name: string): boolean {
  return /^on[A-Z]/.test(name);
}

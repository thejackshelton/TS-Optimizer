/**
 * Constant-folding for JSX prop expressions.
 *
 * Mirrors SWC's `simplify::simplifier` pass (explicitly invoked by the
 * Qwik optimizer's Rust reference at `swc-reference-only/parse.rs:360`)
 * for the subset relevant to JSX prop values: trivial binary / unary /
 * logical / conditional expressions where the operands are primitive
 * literals. Evaluating these at compile time shrinks the emitted
 * `_jsxSorted(...)` calls — `prop={'true' + 1 ? 'true' : ''}` becomes
 * `prop: 'true'` rather than `prop: "true" + 1 ? "true" : ""`.
 *
 * Post-order traversal so nested foldable subtrees collapse from the
 * leaves up; `('true' + 1) ? a : b` first folds the binary expression
 * to a string literal, then the conditional sees a literal test and
 * picks the consequent branch.
 *
 * Conservative — only folds primitive literal operands (string / number
 * / boolean / null / undefined). BigInt, divide-by-zero, and other
 * exotic coercions are left untouched.
 */
import type { AstMaybeNode } from '../../ast-types.js';

/**
 * Try to fold the given expression to a JS primitive value at compile
 * time. Returns `{ folded: true, value }` if the entire subtree
 * collapses to a primitive literal, otherwise `{ folded: false }`.
 */
export function tryFoldExpression(node: AstMaybeNode): FoldResult {
  if (!node) return UNFOLDABLE;
  switch (node.type) {
    case 'Literal': {
      const v = (node as { value?: unknown; bigint?: unknown }).value;
      const bigint = (node as { bigint?: unknown }).bigint;
      if (bigint !== undefined) return UNFOLDABLE;
      if (
        v === null ||
        v === undefined ||
        typeof v === 'string' ||
        typeof v === 'number' ||
        typeof v === 'boolean'
      ) {
        return { folded: true, value: v };
      }
      return UNFOLDABLE;
    }

    case 'UnaryExpression': {
      const u = node as { operator: string; argument: AstMaybeNode };
      const arg = tryFoldExpression(u.argument);
      if (!arg.folded) return UNFOLDABLE;
      const v = arg.value;
      switch (u.operator) {
        case '!': return { folded: true, value: !v };
        case 'typeof': return { folded: true, value: typeof v };
        case 'void': return { folded: true, value: undefined };
        case '-': return typeof v === 'number' ? { folded: true, value: -v } : UNFOLDABLE;
        case '+': return typeof v === 'number' ? { folded: true, value: +v } : UNFOLDABLE;
        case '~': return typeof v === 'number' ? { folded: true, value: ~v } : UNFOLDABLE;
      }
      return UNFOLDABLE;
    }

    case 'BinaryExpression': {
      const b = node as { operator: string; left: AstMaybeNode; right: AstMaybeNode };
      const left = tryFoldExpression(b.left);
      if (!left.folded) return UNFOLDABLE;
      const right = tryFoldExpression(b.right);
      if (!right.folded) return UNFOLDABLE;
      const l = left.value as never;
      const r = right.value as never;
      switch (b.operator) {
        case '+': return { folded: true, value: (l as never) + (r as never) };
        case '-': return typeof l === 'number' && typeof r === 'number' ? { folded: true, value: l - r } : UNFOLDABLE;
        case '*': return typeof l === 'number' && typeof r === 'number' ? { folded: true, value: l * r } : UNFOLDABLE;
        case '/': return typeof l === 'number' && typeof r === 'number' && r !== 0 ? { folded: true, value: l / r } : UNFOLDABLE;
        case '%': return typeof l === 'number' && typeof r === 'number' && r !== 0 ? { folded: true, value: l % r } : UNFOLDABLE;
        case '===': return { folded: true, value: l === r };
        case '!==': return { folded: true, value: l !== r };
        // eslint-disable-next-line eqeqeq
        case '==': return { folded: true, value: l == r };
        // eslint-disable-next-line eqeqeq
        case '!=': return { folded: true, value: l != r };
        case '<': return { folded: true, value: l < r };
        case '>': return { folded: true, value: l > r };
        case '<=': return { folded: true, value: l <= r };
        case '>=': return { folded: true, value: l >= r };
      }
      return UNFOLDABLE;
    }

    case 'LogicalExpression': {
      const lg = node as { operator: string; left: AstMaybeNode; right: AstMaybeNode };
      const left = tryFoldExpression(lg.left);
      if (!left.folded) return UNFOLDABLE;
      const l = left.value;
      switch (lg.operator) {
        case '&&': return l ? tryFoldExpression(lg.right) : left;
        case '||': return l ? left : tryFoldExpression(lg.right);
        case '??': return l === null || l === undefined ? tryFoldExpression(lg.right) : left;
      }
      return UNFOLDABLE;
    }

    case 'ConditionalExpression': {
      const c = node as { test: AstMaybeNode; consequent: AstMaybeNode; alternate: AstMaybeNode };
      const t = tryFoldExpression(c.test);
      if (!t.folded) return UNFOLDABLE;
      return tryFoldExpression(t.value ? c.consequent : c.alternate);
    }
  }
  return UNFOLDABLE;
}

/**
 * Format a folded primitive value as a JS source string suitable for
 * splicing into emitted code. Strings use single quotes to match SWC's
 * preferred output style; other primitives use their canonical form.
 */
export function formatFoldedLiteral(value: unknown): string {
  if (typeof value === 'string') {
    // Single-quoted with minimal escaping. Embedded single quotes get
    // escaped; embedded double quotes are left alone so the output
    // matches SWC's emit style (`prop: 'has "quotes"'`).
    return `'${value.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/\n/g, '\\n').replace(/\r/g, '\\r')}'`;
  }
  if (value === undefined) return 'undefined';
  if (value === null) return 'null';
  if (typeof value === 'number') {
    if (Number.isNaN(value)) return 'NaN';
    if (value === Infinity) return 'Infinity';
    if (value === -Infinity) return '-Infinity';
    return String(value);
  }
  // boolean
  return String(value);
}

export type FoldResult =
  | { folded: true; value: string | number | boolean | null | undefined }
  | { folded: false };

const UNFOLDABLE: FoldResult = { folded: false };

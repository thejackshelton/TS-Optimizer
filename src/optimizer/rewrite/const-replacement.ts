/**
 * Const replacement module for the Qwik optimizer.
 *
 * Replaces isServer/isBrowser/isDev identifiers imported from Qwik packages
 * with boolean literals based on build configuration.
 */

import MagicString from 'magic-string';
import { parseSync } from 'oxc-parser';
import { walk } from 'oxc-walker';
import type {
  AstNode,
  AstParentNode,
  AstProgram,
} from '../../ast-types.js';
import { RAW_TRANSFER_PARSER_OPTIONS } from '../../ast-types.js';
import type { EmitMode } from '../types/types.js';
import type { ImportInfo } from '../extraction/marker-detection.js';

const CONST_SOURCES = [
  '@qwik.dev/core',
  '@qwik.dev/core/build',
  '@builder.io/qwik',
  '@builder.io/qwik/build',
  '@builder.io/qwik-city/build',
];

function isConstSource(source: string): boolean {
  return CONST_SOURCES.includes(source);
}

interface ConstReplacementResult {
  replacedCount: number;
}

/**
 * The `isDev` value implied by an emit mode: dev/hmr builds are dev, prod is
 * not, and any other mode leaves it unset (so const replacement skips `isDev`).
 * Shared so the parent-rewrite path and the inline/hoist body path derive it
 * identically.
 */
export function deriveIsDev(mode: EmitMode | undefined): boolean | undefined {
  if (mode === 'dev' || mode === 'hmr') return true;
  if (mode === 'prod') return false;
  return undefined;
}

/**
 * Build the `localName → literal` fold map from a module's imports. Only
 * `isServer`/`isBrowser` (gated on `isServer`) and `isDev` (gated on `isDev`)
 * that trace to a Qwik package import are included. `isBrowser` is the negation
 * of `isServer`.
 */
export function buildConstReplacementMap(
  importMap: Map<string, ImportInfo>,
  isServer?: boolean,
  isDev?: boolean,
): Map<string, string> {
  const replacements = new Map<string, string>();

  for (const [localName, info] of importMap) {
    if (!isConstSource(info.source)) continue;

    const { importedName } = info;

    if (isServer !== undefined) {
      if (importedName === 'isServer') replacements.set(localName, String(isServer));
      else if (importedName === 'isBrowser') replacements.set(localName, String(!isServer));
    }

    if (isDev !== undefined && importedName === 'isDev') {
      replacements.set(localName, String(isDev));
    }
  }

  return replacements;
}

/**
 * Walk `program` and overwrite every identifier whose name maps to a literal.
 * Skips import-specifier positions (via `importRanges`), member-access property
 * names, and variable-declarator binding names — an identifier in those
 * positions is a name, not a value reference. Returns the number of edits.
 */
function applyReplacements(
  s: MagicString,
  program: AstProgram,
  replacements: Map<string, string>,
  importRanges?: ReadonlySet<string>,
): number {
  let replacedCount = 0;

  walk(program, {
    enter(node: AstNode, parent: AstParentNode) {
      if (node.type !== 'Identifier') return;

      const replacement = replacements.get(node.name);
      if (replacement === undefined) return;

      if (importRanges?.has(`${node.start}:${node.end}`)) return;
      if (parent?.type === 'MemberExpression' && parent.property === node && !parent.computed) return;
      if (parent?.type === 'VariableDeclarator' && parent.id === node) return;
      if (parent?.type === 'ImportSpecifier' && parent.imported === node) return;

      s.overwrite(node.start, node.end, replacement);
      replacedCount++;
    },
  });

  return replacedCount;
}

/**
 * Replace isServer/isBrowser/isDev identifiers with boolean literals.
 * Only replaces identifiers that trace to actual Qwik package imports.
 * Does NOT touch the import declarations — import cleanup is owned by the
 * parent rewrite (processImports + the surviving-imports usage filter); see
 * the note at the end of the function body.
 */
export function replaceConstants(
  s: MagicString,
  program: AstProgram,
  importMap: Map<string, ImportInfo>,
  isServer?: boolean,
  isDev?: boolean,
): ConstReplacementResult {
  const replacements = buildConstReplacementMap(importMap, isServer, isDev);

  if (replacements.size === 0) {
    return { replacedCount: 0 };
  }

  // Collect import specifier positions to skip during walk
  const importRanges = new Set<string>();
  for (const node of program.body) {
    if (node.type === 'ImportDeclaration') {
      for (const spec of node.specifiers) {
        importRanges.add(`${spec.local.start}:${spec.local.end}`);
      }
    }
  }

  const replacedCount = applyReplacements(s, program, replacements, importRanges);

  // No import-side cleanup of the replaced const bindings here. This runs only
  // inside the parent rewrite, *after* `processImports` has already removed
  // every original import and rebuilt the survivors into the preamble; the
  // surviving-imports usage filter then drops any binding the literal
  // substitution above left unreferenced. A removal pass at this point edits
  // the already-removed original range and re-introduces the import into the
  // module body — which surfaced as a duplicate `@qwik.dev/core` import (one
  // trimmed copy in the preamble, one stale copy at body start) that broke
  // bundlers with "identifier already declared".
  return { replacedCount };
}

/**
 * Fold isServer/isBrowser/isDev in a standalone inline/hoist segment body.
 *
 * The parent-rewrite `replaceConstants` edits the parent MagicString, so the
 * segment/smart paths — whose bodies are sliced from that same string — fold
 * for free. The inline/hoist path instead re-emits bodies as plain strings
 * (built from `ext.bodyText`, outside the parent MagicString), so those bodies
 * keep their `isBrowser`/`isServer` references and, with them, dead client
 * branches on a server build. Folding here lets the downstream parent DCE drop
 * `if (false) { … }` blocks and the now-unused import that only the dead branch
 * referenced. Returns the body unchanged when there is nothing to fold or the
 * body cannot be reparsed.
 */
export function foldConstantsInBodyText(
  body: string,
  importMap: Map<string, ImportInfo>,
  isServer?: boolean,
  isDev?: boolean,
): string {
  const replacements = buildConstReplacementMap(importMap, isServer, isDev);
  if (replacements.size === 0) return body;

  let mentionsCandidate = false;
  for (const name of replacements.keys()) {
    if (body.includes(name)) { mentionsCandidate = true; break; }
  }
  if (!mentionsCandidate) return body;

  // Parenthesise so the body (an arrow/function expression) parses as an
  // expression statement; the single wrapping char is stripped back off.
  const wrapped = `(${body})`;
  let parsed;
  try {
    parsed = parseSync('__const_fold__.tsx', wrapped, RAW_TRANSFER_PARSER_OPTIONS);
  } catch {
    return body;
  }
  if (!parsed.program || parsed.errors?.length) return body;

  const s = new MagicString(wrapped);
  const count = applyReplacements(s, parsed.program, replacements);
  if (count === 0) return body;

  return s.toString().slice(1, -1);
}

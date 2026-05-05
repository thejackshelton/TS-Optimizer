# Coding Conventions

**Analysis Date:** 2026-05-05

This document records the conventions actually in force in `src/` and `tests/`.
Use it when adding new code so the new file fits the existing style.

## Module System

**ESM with NodeNext resolution.**

- `package.json` declares `"type": "module"` and `"engines": { "node": ">=20" }`.
- `tsconfig.json` uses `"module": "NodeNext"` + `"moduleResolution": "NodeNext"` with `"target": "ES2022"` and `"strict": true`.
- All relative imports include the `.js` extension (NodeNext requires it):
  ```ts
  // src/optimizer/transform/index.ts
  import { parseWithRawTransfer } from "../utils/parse.js";
  import { extractSegments } from "../extract.js";
  ```
  Approximately 157 import sites in `src/` use this `.js` suffix. **Always include it on relative imports**, even though the source file is `.ts`.
- Bare-package imports use no extension:
  ```ts
  import MagicString from 'magic-string';
  import { walk } from 'oxc-walker';
  import { parseSync, type ParseResult } from 'oxc-parser';
  ```

## File and Directory Naming

**Files:**
- Source files: kebab-case `.ts` — `capture-analysis.ts`, `rewrite-calls.ts`, `transform-session.ts`, `event-capture-promotion.ts`.
- Test files: same kebab-case stem with `.test.ts` suffix — `capture-analysis.test.ts`, `extract.test.ts`.
- The single AST type re-export module is `src/ast-types.ts` (top-level alias hub for `@oxc-project/types`).
- An `index.ts` is used as a barrel only when a directory groups closely related siblings (e.g. `src/optimizer/transform/index.ts`, `src/optimizer/rewrite/index.ts`). Most directories have **no** barrel file; consumers import individual files.

**Directories:**
- `src/optimizer/` — main pipeline.
- `src/optimizer/transform/` — phase 4/5 transform pipeline (JSX, segments, diagnostics).
- `src/optimizer/rewrite/` — parent-module rewriting submodules.
- `src/optimizer/segment-codegen/` — segment-module emission helpers.
- `src/optimizer/utils/` — small reusable helpers (parse, AST type-guards, identifier checks, source-loc, qrl naming).
- `src/hashing/` — SipHash-1-3 + symbol naming.
- `src/testing/` — test utilities reused by tests (snapshot parser, ast-compare, batch-runner, metadata-compare).
- `tests/<area>/` mirrors `src/<area>/` (e.g. `tests/optimizer/extract.test.ts` ↔ `src/optimizer/extract.ts`).

## Identifier Naming

**Functions:** lowerCamelCase, verb-first.
- `extractSegments`, `transformModule`, `analyzeCaptures`, `rewriteParentModule`, `buildDisplayName`, `qwikHash`, `parseSnapshot`, `compareAst`, `compareMetadata`.

**Types and interfaces:** UpperCamelCase.
- `TransformModulesOptions`, `ExtractionResult`, `RewriteContext`, `SegmentAnalysis`, `JsxTransformResult`, `AstCompareResult`.
- AST node aliases use the `Ast` prefix in `src/ast-types.ts`: `AstNode`, `AstProgram`, `AstFunction`, `AstParseResult`, `AstParentNode`, `AstCompatNode`. Treat `Ast*` as the canonical project-internal name; reach for `@oxc-project/types` re-exports only when working with very specific shapes.

**Variables:** lowerCamelCase.
- Local variables, function params, fields all camelCase: `bodyText`, `parentScopeIds`, `closureNode`, `qrlVarNames`, `migrationDecisions`.

**Constants:**
- Module-level frozen tables / well-known regex objects: SCREAMING_SNAKE_CASE — `RAW_TRANSFER_PARSER_OPTIONS` (`src/ast-types.ts`), `ZERO_KEY` (`src/hashing/siphash.ts`), `DIRECTIVE_MARKER` (`src/optimizer/diagnostics.ts`), `DEFAULT_OPTIONS` (`tests/optimizer/snapshot-options.ts`).
- Internal regex constants (built via `magic-regexp`): camelCase — `leadingSquareBracket`, `trailingSquareBracket`, `paddingParam` (re-exported from `src/optimizer/transform/post-process.ts`).

**Generated identifiers (output, not source):**
- QRL variables in emitted code: `q_<symbolName>` (e.g. `q_App_component_HTDRsvUbLiE`). Stripped variants use `q_qrl_<counter>` where `counter = 0xffff0000 + idx*2` (see `preComputeQrlVarNames` in `src/optimizer/rewrite/index.ts`).
- Segment symbol in `prod` mode: `s_<hash>` (see `transformModule` mode-prod block in `src/optimizer/transform/index.ts`).

## TypeScript Style

**Strict mode is on** (`tsconfig.json` `"strict": true`). Conventions:

- Prefer `interface` for object shapes that are part of the public API or appear in many type slots: `TransformModulesOptions`, `TransformOutput`, `ExtractionResult`, `RewriteContext`. See `src/optimizer/types.ts` for the public-API type module.
- Prefer `type` aliases for unions and primitive-ish aliases: `EmitMode = 'dev' | 'prod' | 'lib' | 'hmr'`, `MinifyMode = 'simplify' | 'none'`, `EntryStrategy = { type: 'inline' } | ...`.
- Use `import type { ... }` for type-only imports. Examples in `src/optimizer/extract.ts` lines 12-18.
- Inline `import('...').Type` is used in `src/optimizer/transform/index.ts` for cross-module diagnostic types when adding a top-level import would be redundant.
- Function signatures: parameters typed; return type usually inferred for short helpers, declared explicitly on public/exported functions (`export function transformModule(options: TransformModulesOptions): TransformOutput`).
- Avoid `any`. The project does use a few targeted `any` casts for AST traversal in legacy walks (e.g. `rewriteNoArgMarkers` in `src/optimizer/rewrite/index.ts` accepts `any` for terse recursive walking) — prefer the typed `walk(...)` API from `oxc-walker` for new code instead.

**Type-guard utility helpers** live in `src/optimizer/utils/ast.ts`:
- `isAstNode(value): value is AstCompatNode`
- `isIdentifierNode`, `hasRange`, `isRangedIdentifierNode`
- `isAssignmentPatternNode`, `isPropertyNode`, `isRestElementNode`, `isVariableDeclaratorNode`
- `forEachAstChild(node, visitor, skipKeys?)` — a manual child iterator for cases where the full `oxc-walker` walk is overkill.

Use these guards instead of inline `node.type === 'X'` chains when the same shape is checked in multiple places.

## Imports — Order and Grouping

Observed top-of-file order (e.g. `src/optimizer/transform/index.ts`, `src/optimizer/rewrite/index.ts`, `src/optimizer/extract.ts`):

1. `import type { ... } from '../../ast-types.js';`  — AST type aliases first.
2. Bare-package value imports — `MagicString`, `parseSync`, `walk`, `equal`.
3. `import type { ... } from '...'` for sibling modules.
4. Sibling/relative value imports, grouped roughly by feature area.

Re-exports for backward compatibility live at the **top** of barrel files immediately after imports (see `src/optimizer/rewrite/index.ts` lines 47-66).

**Path aliases:** None configured — `tsconfig.json` has no `paths` map. All imports use real relative paths plus `.js`.

## Regex — magic-regexp, not raw RegExp

The project preferentially uses **`magic-regexp`** (10+ source files) for any non-trivial pattern. Rationale: the patterns are read by humans porting Rust regexes, and the named-grouping API prevents subtle precedence bugs.

Examples:
```ts
// src/optimizer/diagnostics.ts
const TRAILING_COMMENT_CLOSER = createRegExp(
  exactly('*/').and(whitespace.times.any()).and(maybe(exactly('}')))
    .and(whitespace.times.any()).at.lineEnd(),
);

// src/optimizer/context-stack.ts
const dynamicRouteParam = createRegExp(
  exactly('[').and(oneOrMore(char).grouped()).and(']').at.lineStart().at.lineEnd(),
);
```

Inline `/.../` literals are used only for one-off, single-line patterns (e.g. `extract.ts` line 216 for `@jsxImportSource` detection). For new shared patterns, prefer `magic-regexp` and define them as module-level `const`s.

## Path Handling — pathe only

All path manipulation goes through `src/optimizer/path-utils.ts`, which wraps **`pathe`**. Do NOT import Node's `path` module directly in pipeline code — `pathe` normalizes to forward slashes (matching the Rust optimizer's `to_slash_lossy()`).

```ts
// src/optimizer/path-utils.ts
import { basename, dirname, extname, normalize, normalizeString, relative } from 'pathe';

export function getExtension(filePath: string): string { ... }
export function getBasename(filePath: string): string { ... }
export function getFileStem(filePath: string): string { ... }
export function normalizePath(filePath: string): string { ... }
export function computeRelPath(inputPath: string, srcDir: string): string { ... }
export function computeParentModulePath(relPath: string, explicitExtensions?: boolean): string { ... }
export function computeOutputExtension(sourceExt, transpileTs?, transpileJsx?): string { ... }
```

Tests may import `node:path` directly (it's used in `tests/optimizer/convergence.test.ts`, `tests/testing/*`) because tests are platform-aware fixture loaders, not part of the optimizer's deterministic output.

## AST Traversal — oxc-walker enter/leave + ScopeTracker

The optimizer uses three traversal strategies, in roughly this order of preference:

1. **`walk(program, { enter, leave })`** from `oxc-walker` — the workhorse. Used in 10+ files (`extract.ts`, `const-replacement.ts`, `variable-migration.ts`, `transform/jsx.ts`, `transform/module-cleanup.ts`, `signal-analysis.ts`, etc.). Pattern:

   ```ts
   // src/optimizer/extract.ts
   walk(program, {
     enter(node, parent) {
       if (parent) parentMap.set(node, parent);
       // push naming context, detect markers, push results
       if (pushCount > 0) pushedNodes.set(node, pushCount);
     },
     leave(node) {
       const count = pushedNodes.get(node);
       if (count !== undefined) {
         for (let i = 0; i < count; i++) ctx.pop();
         pushedNodes.delete(node);
       }
     },
   });
   ```
   Notes:
   - When state is pushed in `enter`, count it in `pushedNodes` and pop the same count in `leave`. Do not push and forget.
   - `enter(node, parent)` gives the immediate AST parent. If you need transitive ancestry, build a `parentMap: Map<AstNode, AstParentNode>` in `enter` (see `extract.ts` line 213-220).
   - Use `(this as any).skip()` to prune a subtree (see `tests/optimizer/capture-analysis.test.ts` for the test-helper version).

2. **`getUndeclaredIdentifiersInFunction(closureNode)`** from `oxc-walker` — uses oxc-walker's built-in `ScopeTracker` to return identifiers referenced inside a function but not declared (or its params). This is how `analyzeCaptures` detects $-boundary captures (`src/optimizer/capture-analysis.ts:41`). Used in `capture-analysis.ts`, `segment-codegen/import-collection.ts`, `segment-codegen/body-transforms.ts`, `transform/diagnostic-detection.ts`.

3. **`forEachAstChild(node, visitor, skipKeys?)`** from `src/optimizer/utils/ast.ts` — a one-level child iterator that skips meta keys (`type`, `start`, `end`, `loc`, `range` by default). Use when you only want direct children, not a full subtree walk.

A few legacy hand-rolled recursive walks remain (`rewriteNoArgMarkers` in `rewrite/index.ts`, the `renameIdents` and `collectDeclNames` helpers in `testing/ast-compare.ts`). New code should use `walk` instead.

## Text Rewriting — magic-string

**All source-text edits flow through `magic-string`.** Per `DESIGN.md`:

> The idea is that we should hold operations performed in a virtual representation, then at the end we do a write with magic string. This allows us to hold all the AST information and only need to parse once.

Patterns observed:

- **One `MagicString` per parent module rewrite.** `rewriteParentModule` (`src/optimizer/rewrite/index.ts:168`) constructs `const s = new MagicString(source)` once and threads it through a `RewriteContext`. All sub-passes (`processImports`, `applyModeTransforms`, `rewriteCallSites`, `addCaptureWrapping`, `runJsxTransform`, ...) call `s.overwrite(start, end, text)`, `s.appendLeft`, `s.prependRight`, `s.remove`, then a final `s.toString()` happens in `assembleOutput`.
- **`overwrite(start, end, text)`** is the most common operation — replace an exact source span with new text. Use AST `node.start` / `node.end` byte offsets directly.
- **`appendLeft(pos, text)` vs `prependRight(pos, text)`**: pick based on whether subsequent edits at the same position should appear before or after this one. The codebase uses `appendLeft` for `.w([...])` capture-wrap suffixes and `prependRight` for `/*#__PURE__*/` annotations (see `rewriteCallSites` in `rewrite/index.ts:436`).
- **Wrapped sources for nested transforms.** When a body fragment must be parsed and edited independently (e.g. inlining a $() body), use `createTransformSession` / `createFunctionTransformSession` from `src/optimizer/utils/transform-session.ts`. These wrap the source with a synthetic prefix/suffix, parse, build a `MagicString`, and offer `toSource()` to return only the inner edited text.
- **Never re-print from AST.** No `escodegen` / `astring` / `@babel/generator` is used or allowed. Output preserves original formatting except for explicit `s.overwrite` regions.

Example session helper:
```ts
// src/optimizer/utils/transform-session.ts
export function createTransformSession(filename, sourceText, options = {}): TransformSession | null {
  const wrappedSource = wrapperPrefix + sourceText + wrapperSuffix;
  const parseResult = parseWithRawTransfer(filename, wrappedSource);
  if (!parseResult.program || parseResult.errors?.length) return null;
  return {
    sourceText, wrappedSource, wrapperPrefix, wrapperSuffix,
    offset: wrapperPrefix.length,
    program: parseResult.program,
    edits: new MagicString(wrappedSource),
    toSource() { /* slice off wrapper */ },
  };
}
```

## Parsing — Always Through `parseWithRawTransfer`

```ts
// src/optimizer/utils/parse.ts
import { parseSync, type ParseResult } from 'oxc-parser';
import { RAW_TRANSFER_PARSER_OPTIONS } from '../../ast-types.js';

export function parseWithRawTransfer(filename: string, sourceText: string): ParseResult {
  return parseSync(filename, sourceText, RAW_TRANSFER_PARSER_OPTIONS);
}
```

`RAW_TRANSFER_PARSER_OPTIONS = { experimentalRawTransfer: true }` (defined in `src/ast-types.ts`). Use `parseWithRawTransfer` for any new pipeline parse. Direct calls to `parseSync` from `oxc-parser` exist in a few places (`testing/ast-compare.ts`, `rewrite/index.ts`) and are acceptable when you specifically do **not** want raw-transfer (e.g. test-utility re-parses).

## Pipeline / Pass Composition

The big functions follow a "context object + ordered passes" pattern:

```ts
// src/optimizer/rewrite/index.ts: rewriteParentModule
collectExtractedCalleeNames(ctx);
processImports(ctx);
applyModeTransforms(ctx);
resolveNesting(ctx);
preConsolidateRawPropsCaptures(ctx);
ctx.topLevel = extractions.filter((e) => e.parent === null);
preComputeQrlVarNames(ctx);
rewriteCallSites(ctx);
rewriteNoArgMarkers(ctx);
removeUnusedBindings(ctx);
removeDuplicateExports(ctx);
addCaptureWrapping(ctx);
runJsxTransform(ctx);
collectNeededImports(ctx);
buildQrlDeclarations(ctx);
buildInlineSCalls(ctx);
filterUnusedImports(ctx);
const finalCode = assembleOutput(ctx);
```

When adding a new pass, follow the convention:
1. Declare a `function passName(ctx: RewriteContext): void` in the appropriate sibling module.
2. Mutate fields on `ctx` (and `ctx.s`, the shared `MagicString`).
3. Add the call in the right spot in the orchestrator. Document why it must run before / after a specific neighbor (block comments are appreciated).

`transformModule` in `src/optimizer/transform/index.ts` follows the same pattern over a six-phase pipeline:
- Phase 0 repair → Phase 1 extract → Phase 2 capture analysis → Phase 3 migration → Phase 4 rewrite parent → Phase 5 generate segments → Phase 6 suppress diagnostics.

## Comments & JSDoc

- **Module-level JSDoc** at the top of every `src/` file with a 1-3 sentence description, e.g. `extract.ts`, `transform/index.ts`, `rewrite/index.ts`. Add this for new modules.
- **Function JSDoc** for exported functions that are not trivially-named getters. Use `@param` / `@returns` only when the name+types are not enough.
- **Inline block comments** mark Rust-correspondence anchors: `// HASH-01:`, `// HASH-02:`, `// MODE-01:`, `// BENCH-01:`. These tag implementation decisions back to a documented requirement; preserve them when refactoring.
- Keep regular code comments brief; explain "why" not "what".

## Error Handling

The optimizer **does not throw for normal user-input issues**. Bad source code surfaces as `Diagnostic` objects in `TransformOutput.diagnostics`, never as exceptions:

- `emitC02(identName, file, isClass)` — captured local function/class.
- `emitC05(calleeName, qrlName, file)` — `foo$` called but `fooQrl` not exported.
- `emitPassiveConflictWarning(eventName, file)` — passive vs preventdefault conflict on same JSX element.

`Diagnostic` shape (`src/optimizer/types.ts:121`):
```ts
{
  category: 'error' | 'warning',
  code: string,           // 'C02' | 'C05' | 'preventdefault-passive-check' | ...
  file: string,
  message: string,
  highlights: DiagnosticHighlightFlat[] | null,
  suggestions: null,
  scope: 'optimizer',
}
```

`throw new Error(...)` is reserved for **internal invariant violations** (e.g. malformed snapshot frontmatter in `src/testing/snapshot-parser.ts:99,103`).

`try { ... } catch { ... }` is used sparingly to defensively skip non-fatal sub-AST parse failures (e.g. when re-parsing an inlined body that may itself be malformed). Examples:
- `src/optimizer/transform/index.ts:186` — skip extraction body if re-parse fails.
- `src/optimizer/segment-codegen.ts:287`, `src/optimizer/transform/module-cleanup.ts:68/120/188`.

Do **not** swallow errors silently in new code without a comment explaining what failure mode is being tolerated.

## Logging

**No production logger.** No `console.log`/`console.error`/`console.warn` in `src/`. Diagnostics are the only user-visible signal.

Tests use `console.log` for summary output (e.g. `convergence.test.ts:180`, `siphash.test.ts:117`, `optimizer-benchmark.test.ts:191`). That is acceptable; do not push it into `src/`.

## Function & Module Sizing

The codebase has many medium files (200–700 lines) and a few large hot-spots that should be left alone unless explicitly refactored:
- `src/testing/ast-compare.ts` — 3206 lines (a large catalog of AST normalizations; split is non-trivial because each pass shares helpers).
- `src/optimizer/extract.ts` — 690 lines.
- `src/optimizer/transform/index.ts` — 629 lines.
- `src/optimizer/rewrite/index.ts` — 653 lines.

For new functionality, prefer adding a **new sibling file** (e.g. `src/optimizer/transform/<feature>.ts`) and exporting a single `function applyFeature(ctx)` that mutates the context. This keeps existing files from growing.

## Lint / Format Tooling

**None configured.** No `.eslintrc*`, `eslint.config.*`, `.prettierrc*`, `biome.json`, or formatter is committed. Style is enforced by convention (this document) and code review.

The TypeScript compiler does run in strict mode via `tsc` (implicitly via Vitest's transform); type errors are the main automated style gate.

If you add a formatter later, treat existing files as the ground truth — do not mass-reformat without an explicit decision.

## Public-API Stability Rule

`TransformModulesOptions`, `TransformModuleInput`, `TransformOutput`, `TransformModule`, `SegmentAnalysis`, `EntryStrategy`, `MinifyMode`, `EmitMode`, and `Diagnostic` are part of the **NAPI-binding-compatible public API** (`src/optimizer/types.ts`). Their field names, optional/required flags, and value casing must not drift from the SWC optimizer's NAPI shape. Internal-only extensions go on `SegmentMetadataInternal` (which extends `SegmentAnalysis` with optional `paramNames` / `captureNames`).

---

*Convention analysis: 2026-05-05*

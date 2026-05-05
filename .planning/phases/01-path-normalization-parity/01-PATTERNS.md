# Phase 1: path-normalization-parity - Pattern Map

**Mapped:** 2026-05-05
**Files analyzed:** 6 (5 modified + 1 test extended)
**Analogs found:** 6 / 6

## File Classification

| File | Status | Role | Data Flow | Closest Analog | Match Quality |
|------|--------|------|-----------|----------------|---------------|
| `src/optimizer/path-utils.ts` | MODIFIED | utility | transform (string → struct) | (self — extend existing module) + `src/optimizer/marker-detection.ts` (parse-once-return-struct shape) | exact |
| `src/optimizer/types.ts` | MODIFIED | public-API types (additive field) | declarative | (self — extend existing interface `SegmentAnalysis`) + Rust `SegmentData` (`swc-reference-only/parse.rs:56`) | exact |
| `src/optimizer/extract.ts` | MODIFIED | core (segment extraction; consume `PathData` instead of recomputing) | transform | (self — replace lines 202-208 path-recomputation block with field reads) | exact |
| `src/optimizer/transform/index.ts` | MODIFIED | public-API entry / pipeline driver (compute `PathData` once, thread it) | request-response (per-input pipeline) | (self — line 99-100 is the existing single integration site) + `src/optimizer/rewrite/index.ts:168` (one-MagicString-thread-it pattern) | exact |
| `src/optimizer/transform/segment-generation.ts` | MODIFIED | segment-codegen metadata emission | transform (write `path` + `extension`) | (self — two emission sites at `:371-396` and `:791-815`) | exact |
| `tests/optimizer/path-utils.test.ts` | MODIFIED | unit test (extend) | request-response (call helper, assert) | (self — existing 3 test groups at lines 12-33) + `tests/optimizer/extract.test.ts` (vitest describe/it/expect import shape) | exact |

No new files. Five source files modified; one test file extended.

---

## Pattern Assignments

### `src/optimizer/path-utils.ts` (utility, transform)

**Analog:** self — extend the existing module.

**Module-level JSDoc + import block** (`src/optimizer/path-utils.ts:1-8`):

```typescript
/**
 * Shared path string helpers for optimizer transforms.
 *
 * These utilities intentionally operate on normalized string paths to match
 * the existing optimizer behavior and keep path handling deterministic across
 * platforms.
 */
import { basename, dirname, extname, normalize, normalizeString, relative } from 'pathe';
```

**Action for `parsePath` import block:** add `parse`, `join` to the existing `pathe` import set; do not introduce any other dependency. Keep the module-level JSDoc convention; extend to mention the new `parsePath` entry. Do NOT import from `node:path` (CLAUDE.md anti-pattern; the existing module already only uses `pathe`).

**Existing helper that handles the `'.'` → `''` empty-dir convention** (`path-utils.ts:29-32`) — `parsePath`'s `relDir`/`absDir` MUST follow this same normalization (see RESEARCH §Pitfall 3):

```typescript
/** Get the directory component from a slash-delimited path string. */
export function getDirectory(filePath: string): string {
  const dir = dirname(normalizePath(filePath));
  return dir === '.' ? '' : dir;
}
```

**Existing `computeOutputExtension` to REPLACE** (`path-utils.ts:94-108`):

```typescript
/**
 * Compute the output file extension for QRL imports based on transpilation settings.
 * - transpileTs (with or without transpileJsx): .js (TypeScript fully stripped)
 * - transpileJsx only (no transpileTs): .ts (JSX gone, TS remains)
 * - neither: use source extension (.tsx, .ts, etc.)
 */
export function computeOutputExtension(
  sourceExt: string,
  transpileTs?: boolean,
  transpileJsx?: boolean,
): string {
  if (transpileTs) return '.js';
  if (transpileJsx) return '.ts';
  return sourceExt;
}
```

**New signature (per RESEARCH §D-03):**
`computeOutputExtension(pathData: PathData, transpileTs?: boolean, transpileJsx?: boolean, isTypeScript?: boolean, isJsx?: boolean): string`
returning the no-dot form (`'mjs'`, `'js'`, `'jsx'`, `'ts'`).

**Existing `computeRelPath` to DELETE** (`path-utils.ts:44-62`) — replaced by `parsePath(...).relPath`. Its single non-test caller is `src/optimizer/transform/index.ts:99`.

**Pattern for `PathData` interface (mirror Rust `parse_path` field-for-field):**

The shape mirrors `swc-reference-only/parse.rs:922-930`. Use `interface` with `readonly` fields per CLAUDE.md TypeScript-style rule ("Prefer `interface` for object shapes ... use `import type`"). The closest in-codebase precedent for an interface co-located with the function that produces it is `ExtractionResult` in `src/optimizer/extract.ts:38-95` — declared in the same file as `extractSegments`, exported, and threaded through the rest of the pipeline.

**Pattern for adding a `// HASH-XX:` style Rust-correspondence anchor:**

Hashing module already uses Rust-correspondence inline comments (`src/hashing/siphash.ts:11, :26, :29, :32, :43`):

```typescript
const ZERO_KEY: [number, number, number, number] = [0, 0, 0, 0];
// ...
// HASH-02: Hash input is raw concatenated bytes: scope + rel_path + display_name (no separators)
const input = (scope ?? '') + relPath + displayName;

// HASH-01: SipHash-1-3 with keys (0,0,0,0)
```

**Apply this convention to `parsePath`:** annotate field-derivation lines with `// PATH-XX:` anchors that point to `parse.rs:932-961` so future refactors can grep against the Rust source. Also useful: a `// PATH-MATRIX:` anchor on `computeOutputExtension`'s 5-row branch table pointing to `parse.rs:225-232`.

---

### `src/optimizer/types.ts` (public-API types, declarative)

**Analog:** self — extend the existing `SegmentAnalysis` interface.

**Existing module structure** (`src/optimizer/types.ts:1-9`):

```typescript
/**
 * API types for the Qwik optimizer.
 *
 * These types define the public interface for transformModule() and related
 * functions. They must match the NAPI binding interface exactly so the
 * TypeScript optimizer is a drop-in replacement for the SWC optimizer.
 *
 * Source: Qwik optimizer types.ts (verified from GitHub + research)
 */
```

**Existing `SegmentAnalysis` interface** (`src/optimizer/types.ts:65-78`) — the addition site:

```typescript
export interface SegmentAnalysis {
  origin: string;
  name: string;
  entry: string | null;
  displayName: string;
  hash: string;
  canonicalFilename: string;
  extension: string;
  parent: string | null;
  ctxKind: 'eventHandler' | 'function' | 'jSXProp';
  ctxName: string;
  captures: boolean;
  loc: [number, number];
}
```

**Action:** insert `path: string;` between `extension: string;` (line 72) and `parent: string | null;` (line 73). This mirrors the field order in Rust's `SegmentData` struct (per RESEARCH §Existing TS Code State item 5).

`SegmentMetadataInternal` (`src/optimizer/types.ts:87-90`) extends `SegmentAnalysis`, so it picks up the new field automatically — no separate edit needed.

**JSDoc convention to apply:** the file uses single-line field comments only when needed. Per CLAUDE.md "no leading dot on `extension`", add a brief field comment on `extension` clarifying the no-dot convention while editing the surrounding lines (RESEARCH constraint #10).

**Public-API stability note:** the change is additive (new field, no rename, no type change). Per CLAUDE.md "Public-API Stability Rule" and RESEARCH §Open Questions Q1, this is consistent with the frozen-shape constraint because Rust's `SegmentData` already exposes `path` — TS is completing the mirror, not diverging.

---

### `src/optimizer/extract.ts` (core, transform)

**Analog:** self — replace the recomputation block at lines 202-208 with reads from a new `pathData` parameter.

**Existing import line** (`src/optimizer/extract.ts:34`):

```typescript
import { getBasename, getDirectory, getExtension, getFileStem } from './path-utils.js';
```

**Action:** drop unused helpers from this import after the migration. `getBasename`, `getDirectory`, `getFileStem`, `getExtension` calls inside `extractSegments` migrate to `pathData.fileName`, `pathData.relDir`, `pathData.fileStem`, `pathData.extension`. Keep the helpers themselves exported (they are still used by `hashing/naming.ts:96`, `context-stack.ts:31`, etc. — RESEARCH §Scope alarm).

**Existing recomputation block to MIGRATE** (`src/optimizer/extract.ts:202-208`):

```typescript
const relDir = getDirectory(relPath);
const fileStem = getFileStem(relPath) === 'index' && relDir
  ? getBasename(relDir)
  : getBasename(relPath);
const sourceExt = getExtension(relPath) || '.js';
const fileName = getBasename(relPath);
const ctx = new ContextStack(fileStem, relPath, scope, fileName);
```

**Pattern after migration:** locals `relDir`, `fileStem`, `sourceExt`, `fileName` come from `pathData.relDir`, `pathData.fileStem`, `pathData.extension` (note: no leading dot, callers must add one if they require dot-prefixed for legacy reasons), `pathData.fileName`. The `index` → parent-dir-basename heuristic in lines 203-205 stays exactly as-is — that's segment-naming logic, not path-derivation. It still consults the same data, just from `pathData`.

**Existing `determineExtension` helper to DELETE** (`src/optimizer/extract.ts:97-113`):

```typescript
/**
 * Determine the file extension for a segment based on whether its body
 * contains JSX nodes.
 */
function determineExtension(argNode: AstNode, sourceExt: string): string {
  let hasJsx = false;
  walk(argNode, {
    enter(node: AstNode) {
      if (node.type === 'JSXElement' || node.type === 'JSXFragment') {
        hasJsx = true;
      }
    },
  });
  if (hasJsx) return '.tsx';
  if (sourceExt === '.ts') return '.ts';
  return '.js';
}
```

This is a TS-invented secondary extension-deciding code path with no Rust counterpart (RESEARCH §D-03 final paragraph). Delete the function and its two call sites at `:510` and `:599`. Replace the `extension` field in the pushed `ExtractionResult` with the matrix-derived value passed in via context (or computed from `pathData` + flags). Per RESEARCH the responsibility is subsumed by `computeOutputExtension(pathData, ...)`.

**Existing extension assignment in pushed records** (`src/optimizer/extract.ts:392, :414, :510, :533, :599`):

```typescript
const extension = sourceExt;       // line 392 (inlinedQrl branch)
// ...
extension,                         // line 414 (record field)
// ...
const extension = determineExtension(arg, sourceExt);   // line 510 (regular marker)
// ...
extension,                         // line 533 (record field)
// ...
const extension = determineExtension(expr, sourceExt);   // line 599 (JSX-attr branch)
```

**Action:** replace each `const extension = ...` declaration with a single source-of-truth value derived from `pathData`. Confirm with planner whether these records carry the dot-prefixed legacy form (`'.tsx'`) or the new no-dot form (`'tsx'`). RESEARCH §D-03 argues for no-dot end-to-end and for deleting the `replace(leadingDot, "")` strips at `segment-generation.ts:378, :798`.

---

### `src/optimizer/transform/index.ts` (public-API entry, request-response)

**Analog:** self — line 98 is the per-input loop; lines 99-100 are the existing single integration site. Secondary analog: `src/optimizer/rewrite/index.ts:168` ("one MagicString per parent-rewrite, threaded through context") for the "compute once, thread through" idiom.

**Existing per-input loop with ad-hoc path computation** (`src/optimizer/transform/index.ts:98-103`):

```typescript
for (const input of options.input) {
  const relPath = computeRelPath(input.path, options.srcDir);
  const ext = getExtension(relPath);

  if (ext === ".ts" || ext === ".tsx") isTypeScript = true;
  if (ext === ".tsx" || ext === ".jsx") isJsx = true;
```

**Action:** insert `const pathData = parsePath(input.path, options.srcDir);` as line 99 (the new STEP 0). Replace `relPath` and `ext` with `pathData.relPath` and (`'.' + pathData.extension`) where the dot is still needed for legacy comparisons; otherwise switch comparisons to the no-dot form.

**Existing "downgrade extensions" loop to DELETE** (`src/optimizer/transform/index.ts:444-466`):

```typescript
// Save source extensions before downgrading
const sourceExtensions = new Map<string, string>();
for (const extraction of extractions) {
  sourceExtensions.set(extraction.symbolName, extraction.extension);
}

// When JSX will be transpiled, downgrade extensions on extraction results
if (shouldTranspileJsx || shouldTranspileTs) {
  for (const extraction of extractions) {
    if (shouldTranspileJsx) {
      if (extraction.extension === ".tsx")
        extraction.extension = shouldTranspileTs ? ".js" : ".ts";
      else if (extraction.extension === ".jsx")
        extraction.extension = ".js";
      else if (shouldTranspileTs && extraction.extension === ".ts")
        extraction.extension = ".js";
    } else if (shouldTranspileTs) {
      if (extraction.extension === ".ts") extraction.extension = ".js";
      else if (extraction.extension === ".tsx")
        extraction.extension = ".jsx";
    }
  }
}
```

This loop is the historical workaround for not having the Rust matrix. After Phase 1, replace with a single `computeOutputExtension(pathData, options.transpileTs, options.transpileJsx, isTypeScript, isJsx)` call producing one extension value used everywhere downstream (see RESEARCH §Step 4 of D-03). `sourceExtensions` map preservation may or may not still be required — per-extraction inspection: if any consumer reads the *original* extension (e.g. for filename re-derivation), the map stays; if the only purpose was to compensate for the in-place mutation we just deleted, the map goes too.

**Existing `computeOutputExtension` call site** (`src/optimizer/transform/index.ts:401-406`):

```typescript
// Compute output extension early (before `ext` is shadowed by extraction loop)
const qrlOutputExt = computeOutputExtension(
  ext,
  options.transpileTs,
  options.transpileJsx,
);
```

**Action:** update to the new signature `computeOutputExtension(pathData, options.transpileTs, options.transpileJsx, isTypeScript, isJsx)`. The local var `qrlOutputExt` is fine as a name; just ensure it now holds the no-dot string and downstream `+ '.' + ext` concatenations gain the dot back at the use site.

**Existing parent module emission site** (`src/optimizer/transform/index.ts:514-521`):

```typescript
const parentModule: TransformModule = {
  path: relPath,
  isEntry: false,
  code: cleanedCode,
  map: null,
  segment: null,
```

**Action (per RESEARCH §Pitfall 7 and §Files-to-modify item 4):** replace `path: relPath` with the `did_transform`-gated form:

```text
path = (pathData.relDir ? pathData.relDir + '/' : '') +
       (didTransform ? pathData.fileStem + '.' + extension : pathData.fileName)
```

Where `didTransform = (transpileTs && isTypeScript) || (transpileJsx && isJsx)` (Rust `parse.rs:255, :260`).

**Threading-pattern analog:** `RewriteContext` in `src/optimizer/rewrite/rewrite-context.ts:26-65` shows the established pattern for threading shared state through a multi-phase pipeline:

```typescript
export interface RewriteContext {
  source: string;
  relPath: string;
  s: MagicString;
  program: AstProgram;
  extractions: ExtractionResult[];
  originalImports: Map<string, ImportInfo>;
  migrationDecisions?: MigrationDecision[];
  // ... 20+ fields
}
```

**Apply to `SegmentGenerationContext`** (`src/optimizer/transform/segment-generation.ts:207-236`): add `pathData: PathData` as a new field (or replace the existing scalar `relPath: string` field with `pathData: PathData` and have downstream code read `ctx.pathData.relPath`). Either is acceptable per CLAUDE.md "Claude's Discretion" — recommend adding `pathData` and KEEPING `relPath` until atomic migration completes, then dropping `relPath`. RESEARCH §Existing TS Code State recommends atomic, so the final state has just `pathData` on the context.

---

### `src/optimizer/transform/segment-generation.ts` (segment-codegen, transform)

**Analog:** self — two emission sites that build `SegmentMetadataInternal` records.

**First emission site (inline strategy)** (`src/optimizer/transform/segment-generation.ts:371-396`):

```typescript
const segmentAnalysis: SegmentMetadataInternal = {
  origin: ext.origin,
  name: ext.symbolName,
  entry: entryField,
  displayName: ext.displayName,
  hash: ext.hash,
  canonicalFilename: ext.canonicalFilename,
  extension: ext.extension.replace(leadingDot, ""),
  parent: ext.parent,
  ctxKind: ext.ctxKind,
  ctxName: ext.ctxName,
  captures: ext.captures,
  loc: ext.loc,
  captureNames: ext.captureNames,
  paramNames: ext.paramNames,
};

const segmentModule: TransformModule = {
  path: ext.canonicalFilename + ext.extension,
  isEntry: true,
  code: stripped ? generateStrippedSegmentCode(ext.symbolName) : "",
  map: null,
  segment: segmentAnalysis,
  origPath: null,
};
```

**Second emission site (default strategy)** (`src/optimizer/transform/segment-generation.ts:791-815`):

```typescript
const segmentAnalysis: SegmentMetadataInternal = {
  origin: ext.origin,
  name: ext.symbolName,
  entry: entryField,
  displayName: ext.displayName,
  hash: ext.hash,
  canonicalFilename: ext.canonicalFilename,
  extension: ext.extension.replace(leadingDot, ""),
  parent: ext.parent,
  ctxKind: ext.ctxKind,
  ctxName: ext.ctxName,
  captures: ext.captures,
  loc: ext.loc,
  captureNames: ext.captureNames,
  paramNames: ext.paramNames,
};

const segmentModule: TransformModule = {
  path: ext.canonicalFilename + ext.extension,
  isEntry: true,
  code: segmentCode,
  map: null,
  segment: segmentAnalysis,
  origPath: null,
};
```

**Action — both sites:**

1. **Add `path: ctx.pathData.relDir,`** in the `SegmentMetadataInternal` literal (between `extension` and `parent` to mirror the new `SegmentAnalysis` field order).
2. **Replace `extension: ext.extension.replace(leadingDot, "")`** with `extension: ext.extension` (the field is now stored in no-dot form upstream — RESEARCH §Step 4 of D-03). The `leadingDot` strip becomes a no-op once upstream is fixed; remove it to avoid creating a "two writers, two conventions" surface (RESEARCH §Pitfall 2 / TD-10).
3. **Rebuild `segmentModule.path`** per RESEARCH §Architecture-Diagram + Rust `parse.rs:451-461`:

```typescript
// Rust: [&path_str, "/"].concat() prefix when rel_dir non-empty,
// then [&canonical_filename, ".", &extension].concat()
const pathPrefix = ctx.pathData.relDir ? ctx.pathData.relDir + '/' : '';
const segmentModule: TransformModule = {
  path: pathPrefix + ext.canonicalFilename + '.' + ext.extension,
  // ...
};
```

Note the explicit `'.'` between filename and extension (extension is now no-dot).

**Existing `leadingDot` re-export** (`src/optimizer/transform/post-process.ts` — referenced via line 47 of segment-generation.ts):

```typescript
import {
  getManualEntryMap,
  leadingDot,
  // ...
} from "./post-process.js";
```

`leadingDot` may still have other consumers; do not delete the re-export until grep confirms it's no longer used. Within `segment-generation.ts` it can be dropped from the import list once both extension assignments are migrated.

---

### `tests/optimizer/path-utils.test.ts` (unit test, request-response)

**Analog:** self — the file already exists with 3 test groups. Secondary analog: `tests/optimizer/extract.test.ts` for the broader vitest convention used across this test directory.

**Existing test file shape** (`tests/optimizer/path-utils.test.ts:1-33`):

```typescript
import { describe, expect, it } from 'vitest';
import {
  computeRelPath,
  getBasename,
  getDirectory,
  getExtension,
  isRelativePathInsideBase,
  normalizePath,
  stripExtension,
} from '../../src/optimizer/path-utils.js';

describe('path-utils', () => {
  it('normalizes windows-style paths', () => {
    expect(normalizePath('src\\components\\App.tsx')).toBe('src/components/App.tsx');
    expect(getBasename('src\\components\\App.tsx')).toBe('App.tsx');
    expect(getDirectory('src\\components\\App.tsx')).toBe('src/components');
    expect(getExtension('src\\components\\App.tsx')).toBe('.tsx');
    expect(stripExtension('src\\components\\App.tsx')).toBe('src/components/App');
  });

  it('preserves current computeRelPath behavior for paths outside srcDir', () => {
    expect(computeRelPath('src/routes/index.tsx', 'src')).toBe('routes/index.tsx');
    expect(computeRelPath('other/index.tsx', 'src')).toBe('other/index.tsx');
    expect(computeRelPath('src', 'src')).toBe('src');
  });

  it('detects whether a relative import stays within the srcDir-relative tree', () => {
    expect(isRelativePathInsideBase('./styles.css', 'routes/app/index.tsx')).toBe(true);
    expect(isRelativePathInsideBase('../shared/theme.css', 'routes/app/index.tsx')).toBe(true);
    expect(isRelativePathInsideBase('../../../global.css', 'routes/app/index.tsx')).toBe(false);
    expect(isRelativePathInsideBase('../outside.css', 'index.tsx')).toBe(false);
  });
});
```

**Existing convention in this directory** (`tests/optimizer/extract.test.ts:1-3`):

```typescript
import { describe, it, expect } from 'vitest';
import { extractSegments, type ExtractionResult } from '../../src/optimizer/extract.js';
import { generateSegmentCode } from '../../src/optimizer/segment-codegen.js';
```

Note: `extract.test.ts` uses order `describe, it, expect`; `path-utils.test.ts` currently uses `describe, expect, it`. Either is fine; pick one and stick with it for new tests.

**Actions:**

1. **DROP `computeRelPath` from imports** (line 3) and **MIGRATE its 3 assertions** at lines 22-24 into the new `parsePath` test groups (they become assertions on `parsePath(...).relPath`). Per RESEARCH §Test Plan, the semantics change: `parsePath('src/routes/index.tsx', 'src').relPath` returns `'src/routes/index.tsx'` verbatim (Rust does NOT strip `srcDir` from `rel_path`), not `'routes/index.tsx'`. Document this in the test name.
2. **ADD imports for `parsePath`, `qwikHash`** (the latter for D-07 byte-equivalence rows).
3. **ADD a new `describe('parsePath', () => { ... })` block** with one `it()` per row in RESEARCH §Test Plan boundary table (Groups 1-7) covering: `../node_modules/...mjs`, `components/component.tsx`, `./node_modules/qwik-tree/index.qwik.jsx`, `foo/.qwik.mjs` (double-extension), `Makefile` (no-extension defensive), srcDir variants `''`/`.`/`./`, Windows backslash inputs.
4. **ADD a new `describe('parsePath hash byte-equivalence (D-07)', ...)`** with two parameterized rows asserting `qwikHash(undefined, parsePath(...).relPath, contextPortion) === expectedFromSnapshot` for `example_qwik_react` (`x04JC5xeP1U`) and `root_level_self_referential_qrl_inline` (`XMEiO6Rrd3Y`).
5. **PRESERVE the existing 'normalizes windows-style paths' test (lines 12-19)** — it exercises low-level helpers that survive D-01 and is independent of `parsePath`. Keep as-is.
6. **PRESERVE 'detects whether a relative import stays within the srcDir-relative tree' (lines 27-32)** — `isRelativePathInsideBase` is unchanged.

---

## Shared Patterns

### Module-level JSDoc + `// XXX-NN:` Rust-correspondence anchors

**Source:** `src/hashing/siphash.ts:1-30`, `src/optimizer/path-utils.ts:1-7`.
**Apply to:** `path-utils.ts` (new `parsePath`, `PathData`, replacement `computeOutputExtension`).

Every `src/` module has a 1-3 sentence module-level JSDoc (CLAUDE.md "Comments & JSDoc"). Inside the module, lines that map directly to Rust source are tagged with grep-friendly anchors (`// HASH-01:`, `// HASH-02:`, `// MODE-01:`).

```typescript
// from src/hashing/siphash.ts:25-30
// HASH-02: Hash input is raw concatenated bytes: scope + rel_path + display_name (no separators)
const input = (scope ?? '') + relPath + displayName;

// HASH-01: SipHash-1-3 with keys (0,0,0,0)
const result = SipHash13.hash(ZERO_KEY, input);
```

For Phase 1, use `// PATH-NN:` (or `// CONV-01-NN:`) anchors for: `parse_path` field derivations (parse.rs:932-961), the extension matrix (parse.rs:225-232), `parse_filename` rules (parse.rs:687-700), `did_transform` gate (parse.rs:255, :260, :600-605).

### `import type { ... }` for type-only imports

**Source:** `src/optimizer/extract.ts:11-18`, `src/optimizer/transform/index.ts:10-15, :36-40`, `src/optimizer/rewrite/rewrite-context.ts:7-14`.
**Apply to:** any new type imports added during this phase (e.g. importing `PathData` from `path-utils.ts` into `extract.ts`, `transform/index.ts`, `transform/segment-generation.ts`).

```typescript
// from src/optimizer/extract.ts:11-18
import type {
  AstNode,
  AstParentNode,
  AstParseResult,
  AstProgram,
  JSXAttributeItem,
  JSXElementName,
} from '../ast-types.js';
```

### NodeNext relative-import `.js` extension

**Source:** every relative import across `src/` and `tests/`. Examples:

```typescript
// src/optimizer/path-utils.ts uses bare-package import (no extension):
import { basename, dirname, extname, normalize, ... } from 'pathe';

// src/optimizer/extract.ts uses relative imports WITH .js:
import { qwikHash } from '../hashing/siphash.js';
import { ContextStack } from './context-stack.js';

// tests/optimizer/path-utils.test.ts uses .js relative imports:
import { ... } from '../../src/optimizer/path-utils.js';
```

**Apply to:** every new import added in this phase. Bare-package imports (`from 'pathe'`, `from 'vitest'`) carry no extension. Relative imports use `.js` even for `.ts` source files (CLAUDE.md "Module System").

### `interface` for object shapes; `readonly` where immutability is intended

**Source:** `src/optimizer/types.ts:15-50`, `src/optimizer/extract.ts:38-95` (`ExtractionResult`), `src/optimizer/rewrite/rewrite-context.ts:26-65`.

```typescript
// src/optimizer/types.ts:15-33
export interface TransformModulesOptions {
  input: TransformModuleInput[];
  srcDir: string;
  rootDir?: string;
  // ...
}
```

The codebase does not use `readonly` widely on interface fields. `PathData` per RESEARCH §Claude's Discretion should use `readonly` for all fields (immutable struct — never mutate after construction). This is a NEW convention introduced by Phase 1, and is locally justified: the struct is built once at the top of `transformModule` and threaded read-only through the rest of the pipeline. Document the choice with a one-line comment so a future maintainer doesn't take it as a project-wide convention.

### SCREAMING_SNAKE_CASE for module-level frozen tables (NOT applicable here)

**Source:** `src/ast-types.ts:100-102` (`RAW_TRANSFER_PARSER_OPTIONS`), `src/hashing/siphash.ts:11` (`ZERO_KEY`).

```typescript
// src/ast-types.ts:100
export const RAW_TRANSFER_PARSER_OPTIONS: AstRawTransferParserOptions = {
  experimentalRawTransfer: true,
};

// src/hashing/siphash.ts:11
const ZERO_KEY: [number, number, number, number] = [0, 0, 0, 0];
```

**Reach for this if** a constant table is introduced (e.g. an explicit lookup `EXTENSION_MATRIX: Record<string, string>` if the planner prefers a table-driven implementation of the 5-row matrix instead of nested branches). Otherwise, this convention does not apply to Phase 1.

### Vitest test convention

**Source:** `tests/optimizer/extract.test.ts:1-3`, `tests/optimizer/path-utils.test.ts:1-10`, `vitest.config.ts` (test.globals: false).

```typescript
import { describe, it, expect } from 'vitest';
import { ... } from '../../src/optimizer/<module>.js';

describe('<unit-name>', () => {
  it('<behavioral assertion>', () => {
    expect(<call>).toBe(<expected>);
  });
});
```

Globals are disabled (`vitest.config.ts:test.globals: false` per CLAUDE.md). Suite primitives must be imported. Phase 1 unit tests follow this exactly.

---

## No Analog Found

None. Every file in this phase has either a direct self-analog (it is being modified, not created) or a clear co-located precedent in the existing codebase.

---

## Metadata

**Analog search scope:**
- `src/optimizer/path-utils.ts` (full file, 109 lines)
- `src/optimizer/types.ts` (full file, 130 lines)
- `src/optimizer/extract.ts` (lines 1-110, 380-625 — path-relevant blocks)
- `src/optimizer/transform/index.ts` (lines 1-130, 390-521 — per-input loop + emission)
- `src/optimizer/transform/segment-generation.ts` (lines 1-100, 200-280, 360-400, 775-820 — emission sites + ctx interface)
- `src/optimizer/rewrite/rewrite-context.ts` (lines 1-66 — threading-state analog)
- `src/hashing/siphash.ts` (lines 1-50 — `// HASH-XX:` anchor convention + `ZERO_KEY` frozen-const)
- `src/ast-types.ts` (lines 90-103 — `RAW_TRANSFER_PARSER_OPTIONS` frozen-const)
- `src/optimizer/context-stack.ts` (lines 1-45 — neighbor module to extract.ts; uses `getFileStem`)
- `tests/optimizer/path-utils.test.ts` (full file, 33 lines)
- `tests/optimizer/extract.test.ts` (lines 1-60 — vitest convention precedent)

**Files scanned:** 11 source/test files. No new analogs found beyond the self-modification pattern, which is appropriate: this phase is a focused additive/refactor pass within an established module surface.

**Pattern extraction date:** 2026-05-05

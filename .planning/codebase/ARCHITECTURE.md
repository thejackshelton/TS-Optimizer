<!-- refreshed: 2026-05-05 -->
# Architecture

**Analysis Date:** 2026-05-05

## System Overview

The TS Optimizer is a single-function library (`transformModule`) that mirrors
Qwik's Rust/SWC NAPI optimizer. It is consumed by Qwik core's existing Vite
plugin: the plugin hands in a list of source files plus options, and gets back
a list of rewritten parent modules + extracted segment modules + diagnostics.
There is **no CLI** — the library function in `src/optimizer/transform/index.ts`
is the only entry point.

```text
┌──────────────────────────────────────────────────────────────────────┐
│  Qwik Vite Plugin (external) — calls transformModule(options)        │
└─────────────────────────────────┬────────────────────────────────────┘
                                  │ TransformModulesOptions
                                  ▼
┌──────────────────────────────────────────────────────────────────────┐
│  Public API surface (NAPI-shaped, drop-in)                            │
│  `src/optimizer/transform/index.ts` -> transformModule()              │
│  `src/optimizer/types.ts`           -> TransformModulesOptions /      │
│                                         TransformOutput /             │
│                                         SegmentAnalysis /             │
│                                         Diagnostic                    │
└─────────────────────────────────┬────────────────────────────────────┘
                                  │
        ┌─────────────────────────┼─────────────────────────┐
        ▼                         ▼                         ▼
┌──────────────────┐   ┌──────────────────────┐   ┌──────────────────┐
│  Repair / parse  │   │  Extraction phase    │   │  Naming / hash   │
│ `input-repair.ts`│   │ `extract.ts`         │   │ `hashing/        │
│ `utils/parse.ts` │   │ `marker-detection.ts`│   │   siphash.ts`    │
│ (oxc-parser)     │   │ `context-stack.ts`   │   │ `hashing/        │
└────────┬─────────┘   │ (oxc-walker)         │   │   naming.ts`     │
         │             └──────────┬───────────┘   │ (must match Rust │
         │                        │               │  DefaultHasher)  │
         │                        │               └────────┬─────────┘
         │                        ▼                        │
         │             ┌──────────────────────┐            │
         │             │ Capture / migration  │            │
         │             │ `capture-analysis.ts`│            │
         │             │ `variable-migration  │            │
         │             │  .ts`                │            │
         │             │ `loop-hoisting.ts`   │            │
         │             │ `transform/event-    │            │
         │             │  capture-promotion.ts│            │
         │             └──────────┬───────────┘            │
         │                        │                        │
         │                        ▼                        │
         │             ┌──────────────────────┐            │
         │             │ Parent rewrite       │            │
         │             │ `rewrite/index.ts`   │◄───────────┘
         │             │ `rewrite/output-     │
         │             │  assembly.ts`        │
         │             │ `rewrite/raw-props.ts│
         │             │ `rewrite/inline-body │
         │             │  .ts`                │
         │             │ `rewrite/const-      │
         │             │  propagation.ts`     │
         │             │ `transform/jsx*.ts`  │
         │             │ (magic-string text   │
         │             │  edits over AST)     │
         │             └──────────┬───────────┘
         │                        │
         │                        ▼
         │             ┌──────────────────────┐
         │             │ Segment codegen      │
         │             │ `transform/segment-  │
         │             │  generation.ts`      │
         │             │ `segment-codegen.ts` │
         │             │ `segment-codegen/    │
         │             │  body-transforms.ts` │
         │             │ `segment-codegen/    │
         │             │  import-collection.ts│
         │             │ `transform/post-     │
         │             │  process.ts`         │
         │             │ (oxc-transform for   │
         │             │  TS strip)           │
         │             └──────────┬───────────┘
         │                        │
         ▼                        ▼
┌──────────────────────────────────────────────────────────────────────┐
│  TransformOutput { modules[], diagnostics[], isTypeScript, isJsx }   │
│  modules[0] = parent, modules[1..n] = segment files                  │
└──────────────────────────────────────────────────────────────────────┘
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| Public entry | Drives the per-input pipeline, returns NAPI-shaped output | `src/optimizer/transform/index.ts` |
| Public types | NAPI-compatible API contract (must match Rust optimizer) | `src/optimizer/types.ts` |
| AST type re-exports | Centralised oxc/ESTree types + `RAW_TRANSFER_PARSER_OPTIONS` | `src/ast-types.ts` |
| Parser wrapper | Single thin wrapper around `oxc-parser` `parseSync` with raw-transfer flag | `src/optimizer/utils/parse.ts` |
| Input repair | Recover from oxc parse failures that SWC tolerates (unmatched parens, JSX text arrows) | `src/optimizer/input-repair.ts` |
| Marker detection | Find `$`-suffixed callees imported from Qwik packages or custom-inlined | `src/optimizer/marker-detection.ts` |
| Extraction | Walk AST, build `ExtractionResult[]` (positions, body text, ctx, hash, displayName) | `src/optimizer/extract.ts` |
| Context stack | Track naming context during traversal and produce displayName/symbolName | `src/optimizer/context-stack.ts` |
| Hashing | SipHash-1-3 with zero keys; matches Rust `DefaultHasher`. Drives QRL hash + JSX key prefix | `src/hashing/siphash.ts`, `src/hashing/naming.ts`, `src/optimizer/key-prefix.ts` |
| Capture analysis | Identify identifiers crossing `$()` boundary using `oxc-walker.getUndeclaredIdentifiersInFunction` | `src/optimizer/capture-analysis.ts` |
| Variable migration | Decide `move` / `reexport` (`_auto_X`) / `keep` for module-level decls | `src/optimizer/variable-migration.ts` |
| Loop hoisting | Detect `.map`/`for`/`while` enclosing event handlers; emit q:p / q:ps slot params | `src/optimizer/loop-hoisting.ts`, `src/optimizer/transform/event-capture-promotion.ts` |
| Diagnostics | Build C02/C05/preventdefault diagnostics; parse `@qwik-disable-next-line` directives | `src/optimizer/diagnostics.ts`, `src/optimizer/transform/diagnostic-detection.ts` |
| Parent rewrite engine | 11-phase pipeline that emits the rewritten parent file via `MagicString` | `src/optimizer/rewrite/index.ts` |
| Rewrite shared state | Shared mutable bag threaded through every rewrite phase | `src/optimizer/rewrite/rewrite-context.ts` |
| Output assembly | QRL declarations, `_auto_` re-exports, `.s()` calls, TS strip via `oxc-transform` | `src/optimizer/rewrite/output-assembly.ts` |
| Raw-props transform | Rewrite destructured component props into `_rawProps.X` accesses (transform-session-based) | `src/optimizer/rewrite/raw-props.ts` |
| Inline body transform | Body rewrite for `inline`/`hoist` entry strategies | `src/optimizer/rewrite/inline-body.ts` |
| Const propagation | Resolve `const x = literal` from parent into segment captures, drop dead consts | `src/optimizer/rewrite/const-propagation.ts` |
| JSX transform | Convert JSX to `_jsxSorted` / `_jsxSplit` calls with `varProps`/`constProps`, key prefixes, fragments | `src/optimizer/transform/jsx.ts`, `src/optimizer/transform/jsx-children.ts`, `src/optimizer/transform/jsx-elements-core.ts`, `src/optimizer/transform/jsx-props.ts` |
| Signal hoisting | `_wrapProp` / `_fnSignal` derivation for signal-bearing JSX props | `src/optimizer/signal-analysis.ts` |
| Event handlers | Event prop name canonicalisation, passive directives | `src/optimizer/transform/event-handlers.ts` |
| Segment codegen | Per-extraction module text + `SegmentAnalysis` metadata | `src/optimizer/transform/segment-generation.ts`, `src/optimizer/segment-codegen.ts` |
| Segment body transforms | Capture unpacking, signature rewrite, dead-const removal, sync$ inlining, raw-props application | `src/optimizer/segment-codegen/body-transforms.ts` |
| Segment imports | Recollect imports actually referenced after rewrites | `src/optimizer/segment-codegen/import-collection.ts` |
| Post-process | TS strip, isServer/isBrowser const replace, DCE, side-effect simplify, HMR injection, unused-import cleanup on emitted segments | `src/optimizer/transform/post-process.ts`, `src/optimizer/transform/module-cleanup.ts`, `src/optimizer/transform/dead-code.ts` |
| Transform sessions | Parse-once, edit-via-MagicString helper used by raw-props and body transforms | `src/optimizer/utils/transform-session.ts` |
| Strip / strategy / dev | Mode-specific helpers (strip ctx, strip exports, inline strategy, dev QRLs) | `src/optimizer/strip-ctx.ts`, `src/optimizer/strip-exports.ts`, `src/optimizer/inline-strategy.ts`, `src/optimizer/entry-strategy.ts`, `src/optimizer/dev-mode.ts` |
| Snapshot tooling | Parse Rust insta-format `.snap` files and structurally compare ASTs/metadata | `src/testing/snapshot-parser.ts`, `src/testing/ast-compare.ts`, `src/testing/metadata-compare.ts`, `src/testing/batch-runner.ts` |

## Pattern Overview

**Overall:** Single-pass, parse-once, virtual-edit pipeline.

**Key Characteristics:**
- One public function (`transformModule`) — pure, no IO. All file paths are
  treated as strings; `srcDir`/`rootDir` are passed in by the caller.
- ESM-only, NodeNext modules, all internal imports use the `.js` extension.
- The optimizer **never reprints from AST**: it parses with `oxc-parser`,
  records source positions on `ExtractionResult` and AST nodes, and emits
  text edits via `magic-string` (the design directive in `DESIGN.md`).
  TS stripping is delegated to `oxc-transform` only on already-rewritten text.
- Hashing is centrally defined in `src/hashing/siphash.ts` (SipHash-1-3 with
  zero keys) and **must** stay binary-compatible with Rust's `DefaultHasher`
  to keep QRL references resolvable at runtime.
- Per-input control flow is a fixed phase sequence (Phase 0 repair → Phase 1
  extract → Phase 2 capture/migration → Phase 3 promote → Phase 4 rewrite
  parent → Phase 5 generate segments → Phase 6 diagnostic suppression).

## Layers

**Public API layer (`src/optimizer/transform/`):**
- Purpose: Drop-in NAPI-shaped surface
- Location: `src/optimizer/transform/`
- Contains: `index.ts` (`transformModule`), `segment-generation.ts`,
  `module-cleanup.ts`, `post-process.ts`, `dead-code.ts`,
  `diagnostic-detection.ts`, JSX transform modules, event handlers,
  `event-capture-promotion.ts`
- Depends on: every other layer
- Used by: external Qwik Vite plugin (and tests in `tests/`)

**Core extraction / analysis layer (`src/optimizer/`, top-level files):**
- Purpose: Identify segments and compute their metadata
- Location: `src/optimizer/*.ts`
- Contains: `extract.ts`, `marker-detection.ts`, `capture-analysis.ts`,
  `variable-migration.ts`, `context-stack.ts`, `loop-hoisting.ts`,
  `signal-analysis.ts`, `diagnostics.ts`, `entry-strategy.ts`,
  `inline-strategy.ts`, `strip-ctx.ts`, `strip-exports.ts`, `dev-mode.ts`,
  `const-replacement.ts`, `key-prefix.ts`, `path-utils.ts`,
  `rewrite-calls.ts`, `rewrite-imports.ts`, `input-repair.ts`,
  `segment-codegen.ts`
- Depends on: `utils/`, `hashing/`, `ast-types.ts`
- Used by: `transform/` and `rewrite/`

**Rewrite engine layer (`src/optimizer/rewrite/`):**
- Purpose: Mutate the parent module text using shared `RewriteContext`
- Location: `src/optimizer/rewrite/`
- Contains: `index.ts` (the 11-phase pipeline), `rewrite-context.ts`,
  `output-assembly.ts`, `raw-props.ts`, `inline-body.ts`,
  `const-propagation.ts`
- Depends on: core layer + `utils/transform-session.ts`
- Used by: `transform/index.ts` (`rewriteParentModule`)

**Segment codegen layer (`src/optimizer/segment-codegen.ts` + `segment-codegen/`):**
- Purpose: Produce per-segment file text plus `SegmentAnalysis`
- Location: `src/optimizer/segment-codegen.ts`,
  `src/optimizer/segment-codegen/body-transforms.ts`,
  `src/optimizer/segment-codegen/import-collection.ts`
- Depends on: core layer, `utils/transform-session.ts`
- Used by: `src/optimizer/transform/segment-generation.ts`

**Utilities layer (`src/optimizer/utils/`):**
- Purpose: Tiny, dependency-free helpers (parsing wrapper, AST shape guards,
  binding patterns, transform sessions, text scanning, path normalisation)
- Location: `src/optimizer/utils/`
- Contains: `parse.ts`, `ast.ts`, `binding-pattern.ts`,
  `identifier-name.ts`, `module-symbols.ts`, `props-field-rewrite.ts`,
  `qrl-naming.ts`, `qwik-packages.ts`, `source-loc.ts`, `text-scanning.ts`,
  `transform-session.ts`
- Depends on: only `ast-types.ts` and external packages
- Used by: every other layer in `src/optimizer/`

**Hashing layer (`src/hashing/`):**
- Purpose: Replicate Rust hash algorithm and Qwik symbol naming exactly
- Location: `src/hashing/`
- Contains: `siphash.ts`, `naming.ts`, `siphash13.d.ts` (ambient typings for
  the JS `siphash` package's SipHash13 export)
- Depends on: `siphash` package, `optimizer/path-utils.ts` (basename only)
- Used by: `extract.ts`, `context-stack.ts`, `key-prefix.ts`

**Testing helpers layer (`src/testing/`):**
- Purpose: Parse Rust insta `.snap` fixtures and structurally compare output
- Location: `src/testing/`
- Contains: `snapshot-parser.ts`, `ast-compare.ts`, `metadata-compare.ts`,
  `batch-runner.ts`
- Depends on: `oxc-parser`, `fast-deep-equal`
- Used by: `tests/optimizer/convergence.test.ts`,
  `tests/optimizer/snapshot-batch.test.ts`, etc.

## Data Flow

### Primary Request Path: `transformModule(options)` per input file

Each `TransformModuleInput` runs through phases 0-5 in order; phase 6 runs
once over all inputs. All references below point at `src/optimizer/transform/index.ts`.

1. **Phase 0 — repair input** (`index.ts:106`): `repairInput()` retries
   parsing with targeted source rewrites if oxc fails on SWC-recoverable
   syntax (`src/optimizer/input-repair.ts`).
2. **Phase 1 — extract segments** (`index.ts:112`): `extractSegments()` walks
   the AST with `oxc-walker`, finds marker calls (`$`-suffixed callees from
   Qwik packages or custom inlined), and produces `ExtractionResult[]` with
   `callStart/callEnd/calleeStart/calleeEnd/argStart/argEnd`, body text,
   ctx kind/name, displayName, hash and canonical filename
   (`src/optimizer/extract.ts`).
3. **Early exit** (`index.ts:123`): if no extractions and no JSX to transpile,
   emit a passthrough module via `buildPassthroughModule()`
   (`src/optimizer/transform/module-cleanup.ts`) and continue.
4. **Phase 2 — collect imports & analyse captures** (`index.ts:142-259`):
   `collectImports()` (`src/optimizer/marker-detection.ts`),
   `collectScopeIdentifiers()` + `analyzeCaptures()`
   (`src/optimizer/capture-analysis.ts`) plus a per-extraction body parse to
   build `closureNodes` / `bodyScopeIds` / `bodyPrograms` maps. Captures are
   filtered against `classifyDeclarationType()` to drop function/class
   declarations.
5. **Phase 2b — promote event-handler captures** (`index.ts:282-316`):
   `promoteEventHandlerCaptures()`, `unifyParameterSlots()`,
   `buildElementCaptureMap()` from
   `src/optimizer/transform/event-capture-promotion.ts` realise the q:p / q:ps
   delivery mechanism and loop hoisting (using
   `src/optimizer/loop-hoisting.ts`).
6. **Phase 2c — diagnostics (C02)** (`index.ts:318`):
   `detectC02Diagnostics()` (`src/optimizer/transform/diagnostic-detection.ts`).
7. **Phase 3 — variable migration** (`index.ts:331-413`):
   `collectModuleLevelDecls()` and `analyzeMigration()`
   (`src/optimizer/variable-migration.ts`) decide `move` / `reexport` / `keep`
   per module-level declaration.
8. **Prod symbol rename** (`index.ts:421`): in `mode === 'prod'`, every
   non-inlinedQrl extraction gets `symbolName = 's_' + hash`.
9. **Phase 4 — rewrite parent module** (`index.ts:468`):
   `rewriteParentModule()` (`src/optimizer/rewrite/index.ts`) runs an 11-step
   in-place edit on a single `MagicString`:
   `collectExtractedCalleeNames` → `processImports` → `applyModeTransforms` →
   `resolveNesting` → `preConsolidateRawPropsCaptures` →
   `preComputeQrlVarNames` → `rewriteCallSites` → `rewriteNoArgMarkers` →
   `removeUnusedBindings` → `removeDuplicateExports` → `addCaptureWrapping`
   → `runJsxTransform` → `collectNeededImports` → `buildQrlDeclarations` →
   `buildInlineSCalls` → `filterUnusedImports` → `assembleOutput`. All
   accumulated state lives on `RewriteContext`
   (`src/optimizer/rewrite/rewrite-context.ts`).
10. **Phase 4b — DCE + unused imports on parent** (`index.ts:508-513`):
    `applySegmentDCE()` and `removeUnusedImports()`.
11. **Phase 4c — diagnostics (C05, passive/preventdefault)**
    (`index.ts:524-540`).
12. **Phase 5 — generate segment modules** (`index.ts:606`):
    `generateAllSegmentModules(segmentCtx)`
    (`src/optimizer/transform/segment-generation.ts`) produces a
    `TransformModule` per extraction. Each segment is built by
    `generateSegmentCode()` (`src/optimizer/segment-codegen.ts`), then run
    through `postProcessSegmentCode()`
    (`src/optimizer/transform/post-process.ts`) for TS strip via
    `oxc-transform`, const replace, DCE, side-effect simplification, HMR
    injection, and unused-import cleanup. `resolveEntryField()`
    (`src/optimizer/entry-strategy.ts`) decides the `entry` metadata field.
13. **Phase 6 — diagnostic suppression** (`index.ts:611-620`):
    `parseDisableDirectives()` + `filterSuppressedDiagnostics()` from
    `src/optimizer/diagnostics.ts` honour `@qwik-disable-next-line` comments.
14. **Return** (`index.ts:622`): `{ modules, diagnostics, isTypeScript, isJsx }`.

### Secondary Flow — Hashing

1. `extract.ts` builds the displayName via `ContextStack.getDisplayName()`
   (`src/optimizer/context-stack.ts`).
2. `ContextStack.getSymbolName()` calls `buildSymbolName()` in
   `src/hashing/naming.ts`, which calls `qwikHash()` in
   `src/hashing/siphash.ts`.
3. `qwikHash()` runs `SipHash13.hash([0,0,0,0], scope+relPath+contextPortion)`,
   converts to little-endian u64 bytes, base64url-encodes, drops padding, and
   replaces `-`/`_` with `0` to produce the 11-character symbol hash.
4. `src/optimizer/key-prefix.ts` uses the same SipHash-1-3 of the file path
   to derive the 2-character JSX key prefix.

### Secondary Flow — Transform Sessions (recent refactor)

`src/optimizer/utils/transform-session.ts` introduces `TransformSession` /
`FunctionTransformSession`, which: (a) optionally wrap source text with a
prefix/suffix, (b) parse it once with oxc raw-transfer, (c) provide a
`MagicString` `edits` buffer plus helpers (`insertFunctionBodyPrologue`,
`replaceFunctionParams`), and (d) `toSource()` strips the wrapper. Used by
`src/optimizer/rewrite/raw-props.ts` and
`src/optimizer/segment-codegen/body-transforms.ts` to replace earlier
parse-twice / regex-rewrite paths (commit 85a9059).

**State Management:**
- No global mutable state. All per-input state lives in local variables of
  `transformModule` and on the `RewriteContext` object.
- `ExtractionResult` is mutated in place across phases (e.g. `parent`,
  `captureNames`, `paramNames`, `captures`, `propsFieldCaptures`,
  `constLiterals`, `symbolName` in prod mode).
- The only environment variable read at runtime is `PERF_TRACE` (used by
  `tests/benchmark/profile-deep.test.ts`).

## Key Abstractions

**`ExtractionResult` (`src/optimizer/extract.ts:38-95`):**
- Purpose: One row per detected `$()` boundary; carries everything later
  phases need (positions, body text, ctx kind, hash, captures, etc.)
- Pattern: Mutable record passed through every phase

**`RewriteContext` (`src/optimizer/rewrite/rewrite-context.ts`):**
- Purpose: Shared mutable bag for the parent-rewrite pipeline; replaces a
  20+ parameter call signature.
- Pattern: Thread-local context object passed to every internal phase
  function

**`TransformSession` (`src/optimizer/utils/transform-session.ts`):**
- Purpose: Parse-once + magic-string edit buffer with optional source
  wrapping (e.g. `const __rl__ = ` prefix to make a free-standing expression
  parseable).
- Pattern: Lightweight object factory (`createTransformSession`,
  `createFunctionTransformSession`) returning typed records with
  helper functions that take the session.

**`ContextStack` (`src/optimizer/context-stack.ts`):**
- Purpose: Stack of naming context entries used to derive `displayName`
  during AST walk; integrates with hashing to produce stable `symbolName`.
- Pattern: Class with `push` / `pop` / `peek` / `pushDefaultExport`,
  encapsulating bracket-route stem extraction.

**`SegmentAnalysis` (`src/optimizer/types.ts:65-78`):**
- Purpose: Public per-segment metadata that ships back to the Vite plugin;
  must match the NAPI binding's shape exactly.
- Pattern: Plain interface; `SegmentMetadataInternal` extends it with
  optional snapshot-only fields (`paramNames`, `captureNames`).

## Entry Points

**Library entry — `transformModule()`:**
- Location: `src/optimizer/transform/index.ts:90`
- Triggers: External Qwik Vite plugin call; in this repo, every test file
  imports it directly (e.g. `tests/optimizer/transform.test.ts:9`,
  `tests/optimizer/convergence.test.ts:17`,
  `tests/benchmark/optimizer-benchmark.test.ts:25`).
- Responsibilities: Run the full per-input pipeline and return
  `TransformOutput`.

**There is no CLI.** No `bin` field in `package.json`, no `src/cli.ts`, no
shebang script. The only npm scripts are `test` and `test:watch`
(`package.json:11-13`).

## Architectural Constraints

- **API shape is frozen:** `TransformModulesOptions`, `TransformOutput`,
  `TransformModule`, `SegmentAnalysis`, `Diagnostic`, `EntryStrategy`,
  `MinifyMode`, `EmitMode` (`src/optimizer/types.ts`) must match the Rust
  NAPI module. Field names, optionality, and value shapes (e.g.
  `loc: [number, number]`, `ctxKind: 'eventHandler' | 'function' | 'jSXProp'`)
  are load-bearing.
- **Hash stability:** `qwikHash()` and `escapeSymbol()` / `buildDisplayName()`
  / `buildSymbolName()` (`src/hashing/`) must reproduce Rust's
  `DefaultHasher` byte-for-byte. The hash is keyed `(0,0,0,0)` and the input
  is the raw concatenation `scope + relPath + displayName` with no
  separators. The same SipHash drives JSX key prefixes
  (`src/optimizer/key-prefix.ts`).
- **Threading:** Single-threaded JS; no worker threads, no async parsing.
  `parseSync` is used everywhere.
- **Global state:** No module-level mutable state. The only caches are local
  to a single function call (`wholeWordPatternCache` in
  `src/optimizer/transform/post-process.ts:117`, `discoveredFiles` inside
  `tests/benchmark/optimizer-benchmark.test.ts:56`).
- **Path handling:** All paths are normalised to forward-slash via
  `pathe` in `src/optimizer/path-utils.ts` to match Rust's
  `to_slash_lossy()`. Never use Node's `path` module directly.
- **Module resolution:** ESM only (`"type": "module"` in `package.json`),
  TS `module: NodeNext` — every relative internal import **must** include the
  `.js` extension (e.g. `import { foo } from './bar.js'` even though the
  source is `bar.ts`).
- **Parser options:** Always parse with
  `RAW_TRANSFER_PARSER_OPTIONS = { experimentalRawTransfer: true }`
  (`src/ast-types.ts:100`); routed through `parseWithRawTransfer` in
  `src/optimizer/utils/parse.ts`.

## Anti-Patterns

### Reprinting from AST

**What happens:** Generating output by calling a code generator on the AST
(e.g. astring, escodegen, `@babel/generator`).
**Why it's wrong:** Loses original formatting, comments and source
positions; defeats the purpose of using `oxc-parser` + `magic-string`.
**Do this instead:** Edit the original text in place via `MagicString` using
the `start`/`end` offsets recorded on AST nodes
(see `src/optimizer/rewrite/index.ts:168-220` and
`src/optimizer/utils/transform-session.ts`). `oxc-transform` is acceptable
**only** as a final TS-stripping pass over already-rewritten text
(`src/optimizer/transform/post-process.ts:188`,
`src/optimizer/rewrite/output-assembly.ts:9`).

### Re-parsing inside loops

**What happens:** Calling `parseSync()` on the same input multiple times
within a phase.
**Why it's wrong:** oxc parsing dominates wall time; re-parsing is the main
perf trap (motivation for the transform-session refactor in commit 85a9059
and the `existingProgram` parameter on `rewriteParentModule`).
**Do this instead:** Parse once at the top of `transformModule`, thread the
`AstProgram` through (`src/optimizer/transform/index.ts:136-141`,
`src/optimizer/rewrite/index.ts:166`,
`src/optimizer/rewrite/index.ts:169`). For ad-hoc sub-parses (e.g. closure
bodies), use a `TransformSession` so the parse + edit buffer are paired.

### Walking AST nodes with hand-written `walk()` recursion

**What happens:** Defining a local recursive `walk(node)` that iterates
`Object.keys(node)`.
**Why it's wrong:** Misses scope tracking (used to exist; e.g.
`src/optimizer/rewrite/index.ts:480-509`'s `rewriteNoArgMarkers` still does
this). Easy to break under raw-transfer when AST shapes have non-standard
fields.
**Do this instead:** Use `walk` / `getUndeclaredIdentifiersInFunction` from
`oxc-walker`, plus `forEachAstChild` from `src/optimizer/utils/ast.ts` for
shallow iteration that respects the metadata-key skip set.

### Reading `path.basename` / `path.extname` from Node's `path`

**What happens:** Importing from `node:path` directly.
**Why it's wrong:** Behaves OS-dependently (backslashes on Windows); Rust's
optimizer always uses forward-slash paths.
**Do this instead:** Import from `pathe` via
`src/optimizer/path-utils.ts` helpers (`getExtension`, `getBasename`,
`getDirectory`, `getFileStem`, `normalizePath`, `computeRelPath`,
`computeParentModulePath`, `computeOutputExtension`).

### Reading bytes / using `crypto` / a different hash

**What happens:** Substituting `node:crypto` SHA-256, MurmurHash, FNV, or a
custom SipHash implementation for Qwik symbol hashing.
**Why it's wrong:** Breaks runtime QRL resolution — emitted segment files
are addressed by hash and must agree with what other Qwik tools emit.
**Do this instead:** Always go through `qwikHash()` in
`src/hashing/siphash.ts`, which uses the upstream `siphash` npm package's
SipHash-1-3 export (typed by `src/hashing/siphash13.d.ts`).

## Error Handling

**Strategy:** Soft failure for parse errors; structured Diagnostics for
optimizer rule violations.

**Patterns:**
- `repairInput()` (`src/optimizer/input-repair.ts`) treats an empty parse
  result with errors as recoverable and returns the original source if no
  repair strategy applies.
- Per-extraction body parses are wrapped in `try { ... } catch { /* skip */ }`
  (`src/optimizer/transform/index.ts:160-188`) so a single bad closure does
  not abort the whole module.
- `compareAst()` (`src/testing/ast-compare.ts:20-50`) retries with a `.tsx`
  filename if `.js`/`.ts` parsing fails because the snapshot contains JSX.
- Optimizer rule violations (C02, C05, preventdefault/passive conflict)
  surface as `Diagnostic` objects in `TransformOutput.diagnostics` rather
  than thrown errors (`src/optimizer/diagnostics.ts`,
  `src/optimizer/transform/diagnostic-detection.ts`).
- `@qwik-disable-next-line C02` and similar comments suppress diagnostics
  via `parseDisableDirectives()` + `filterSuppressedDiagnostics()`
  (`src/optimizer/diagnostics.ts`).

## Cross-Cutting Concerns

**Logging:** None. There is no logger; the optimizer is silent. The only
diagnostic-style output is the structured `Diagnostic[]` returned in
`TransformOutput`.

**Validation:** Type-driven only — `TransformModulesOptions` is enforced by
the public types in `src/optimizer/types.ts`. No runtime schema validation.

**Authentication:** Not applicable (pure compile-time library).

**Module resolution:** All internal imports use `.js` extensions
(NodeNext + ESM). External SDKs are imported normally.

**Performance instrumentation:** A handful of files honour the `PERF_TRACE`
env var (`src/optimizer/transform/index.ts` and friends emit timing when
set; see `tests/benchmark/profile-deep.test.ts`). Benchmarks gate against
SWC: monorepo within 1.15× and worst-case file within 1.5× SWC wall time
(`tests/benchmark/optimizer-benchmark.test.ts:37-39`).

---

*Architecture analysis: 2026-05-05*

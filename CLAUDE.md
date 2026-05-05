<!-- GSD:project-start source:PROJECT.md -->
## Project

**Qwik Optimizer (TypeScript)**

A drop-in TypeScript replacement for Qwik's Rust/SWC optimizer. It takes
Qwik source files containing `$()` boundaries, extracts lazy-loadable
segments, computes captures, generates QRLs, and emits transformed output
that is AST-semantically equivalent to what Qwik's existing SWC optimizer
produces, so QRL hashes resolve correctly and existing Qwik apps run
unchanged. Consumed by Qwik core's existing Vite plugin via the same
NAPI function signature.

**Core Value:** AST-semantic equivalence with the SWC reference optimizer. Whitespace,
import ordering, and other formatting differences don't count. What
matters is that the parsed AST is semantically equivalent — that's what
guarantees correct Qwik runtime behavior at hydration/lazy-load time.
Convergence tests prove this by parsing both outputs with oxc-parser
and deep-equal'ing the normalized ASTs.

### Constraints

- **API compatibility**: Drop-in for the existing NAPI module — same function signature, same output shape — so Qwik core's Vite plugin consumes it without changes.

- **AST-semantic parity**: Output must be AST-semantically equivalent to what the SWC optimizer produces. Convergence tests parse both expected (`match-these-snaps/`) and actual (`ts-output/`) and compare normalized ASTs — whitespace and import order are ignored by design. Semantic identity is what guarantees correct Qwik runtime behavior.

- **QRL hash parity**: For any given source file, the QRL symbol hashes emitted by the TS optimizer must equal the hashes the original Rust/SWC optimizer would emit for the same input. This requires both the same algorithm (SipHash-1-3 with keys (0,0), matching Rust's `std::collections::hash_map::DefaultHasher`) and byte-identical hash inputs (path normalization, identifier serialization, segment numbering must mirror the Rust impl). A wrong hash breaks QRL resolution in apps already deployed against SWC-generated hashes.

- **No double codebase**: Single TS implementation. The Rust source under `swc-reference-only/` is reference material only; no parallel build pipeline.
<!-- GSD:project-end -->

<!-- GSD:stack-start source:codebase/STACK.md -->
## Technology Stack

## Languages
- TypeScript ~5.7 (resolved 5.9.3) — All source under `src/` and tests under `tests/`. Strict mode enabled in `tsconfig.json`.
- JSX/TSX — Parsed and emitted by the optimizer; `tsconfig.json` sets `"jsx": "react-jsx"` for any in-repo TSX.
- Rust (reference only) — `swc-reference-only/` contains the upstream SWC optimizer source as a read-only reference. NOT compiled or executed by this project.
## Runtime
- Node.js >= 20 LTS (declared in `package.json` `engines.node`).
- ESM-only (`"type": "module"` in `package.json`; `"module": "NodeNext"` in `tsconfig.json`).
- Native NAPI bindings (oxc-parser, oxc-transform) require Node `^20.19.0 || >=22.12.0` per oxc binding manifests.
- pnpm 10.25.0 (declared via `"packageManager"` field in `package.json`).
- Lockfile: present (`pnpm-lock.yaml`, lockfileVersion 9.0).
## Frameworks
- None — this is a library. No web/server framework.
- vitest ^4.1.4 (resolved 4.1.4) — Test runner, assertions, and coverage. Config: `vitest.config.ts`. Picks up `tests/**/*.test.ts`, runs with `globals: false` (explicit imports only).
- TypeScript compiler 5.9.3 — Type checking only. There is currently no build script. `tsconfig.json` declares `outDir: "dist"` with `declaration`, `declarationMap`, and `sourceMap` enabled, but `dist/` is gitignored and `package.json` defines no `build` / `prepublish` scripts. The package is `private: true` and consumed in-repo only.
- oxc-transform ^0.124.0 — Used at runtime by the optimizer itself for TS-stripping the generated parent/segment modules. Also serves as the only "downlevel" tool in the project; no Babel, no tsc emit, no esbuild.
## Key Dependencies
- `magic-string` ^0.30.21 — Surgical source text mutation. Drives the entire output assembly pipeline (`src/optimizer/utils/transform-session.ts`, `src/optimizer/rewrite/index.ts`, `src/optimizer/segment-codegen.ts`, etc.). Avoids re-printing from AST.
- `magic-regexp` ^0.11.0 — Type-safe regex builder. Used pervasively for tokenizing and rewriting source text where AST traversal would be overkill. Hot paths in `src/optimizer/segment-codegen/body-transforms.ts`, `src/optimizer/transform/post-process.ts`, `src/optimizer/transform/dead-code.ts`, `src/hashing/siphash.ts`.
- `pathe` ^2.0.3 — Cross-platform path normalization (forward-slash). Used in `src/optimizer/path-utils.ts` to match the Rust optimizer's `to_slash_lossy()` semantics; affects hash inputs so platform-stable behavior is required.
- `siphash` ^1.1.0 (resolved 1.2.0) — SipHash-1-3 implementation. Compatibility-critical: must produce byte-identical hashes to Rust's `std::collections::hash_map::DefaultHasher` (zero keys). Imported via subpath `siphash/lib/siphash13.js` in `src/hashing/siphash.ts`. Local ambient declaration: `src/hashing/siphash13.d.ts`.
- `oxc-parser` ^0.124.0 — ESTree-conformant TS/TSX/JSX parser via Rust NAPI. Sole entry: `src/optimizer/utils/parse.ts` calls `parseSync(filename, sourceText, RAW_TRANSFER_PARSER_OPTIONS)`. Raw-transfer mode (`experimentalRawTransfer: true`) is enabled in `src/ast-types.ts`.
- `oxc-transform` ^0.124.0 — TS syntax stripping via NAPI. Used in `src/optimizer/rewrite/output-assembly.ts`, `src/optimizer/transform/post-process.ts`, and `src/optimizer/transform/module-cleanup.ts` (all imported as `transformSync as oxcTransformSync`).
- `oxc-walker` ^0.7.0 — AST traversal with built-in scope tracking. Imported as `walk` and `getUndeclaredIdentifiersInFunction` across the optimizer (capture analysis, JSX rewrite, segment codegen, diagnostic detection).
- `@oxc-project/types` 0.124.0 (pinned, no caret) — ESTree node type definitions for oxc. Re-exported from `src/ast-types.ts`. Pinned to match `oxc-parser`'s emitted shapes exactly.
- `fast-deep-equal` ^3.1.3 — Structural AST equality for convergence comparisons. Used by `src/testing/ast-compare.ts`, `tests/optimizer/failure-families.test.ts`, `tests/optimizer/convergence-breakdown.test.ts`.
- `@types/node` ^25.6.0 — Node.js type definitions for `node:fs`, `node:path`, `node:module`, `node:url`, `node:child_process` used in tests and the batch runner.
## Configuration
- `target: ES2022`
- `module: NodeNext`, `moduleResolution: NodeNext`
- `strict: true`
- `declaration: true`, `declarationMap: true`, `sourceMap: true`
- `outDir: dist`, `rootDir: .`
- `esModuleInterop: true`, `skipLibCheck: true`, `forceConsistentCasingInFileNames: true`
- `resolveJsonModule: true`, `isolatedModules: true`
- `jsx: react-jsx`
- `include`: `src/**/*.ts`, `tests/**/*.ts`
- `exclude`: `node_modules`, `dist`
- `test.include`: `tests/**/*.test.ts`
- `test.globals`: `false` (suite primitives must be imported)
- No coverage thresholds configured.
- No setup files.
- `"name": "qwik-optimizer-ts"`, `"version": "0.0.1"`, `"private": true`
- `"type": "module"`, `"engines": { "node": ">=20" }`
- No `main` / `exports` / `bin` / `types` entries — not published; consumed via direct `import` from `src/optimizer/transform/index.js` in tests/benchmarks.
- `QWIK_SWC_BINDING` — Absolute path to the upstream Qwik native NAPI optimizer binding (`.node` file or its loader). Read in `tests/benchmark/optimizer-benchmark.test.ts` to enable the SWC vs TS comparison benchmarks. When unset/empty, the benchmark suite is registered as the skipped placeholder `BENCH-00: QWIK_SWC_BINDING was not set, skipping benchmark tests.`
- `BENCH` — Set to `1` to run the benchmark suites (`BENCH-01` full monorepo, `BENCH-02` worst-case single file). Otherwise both are wrapped in `describe.skip`. See `tests/benchmark/optimizer-benchmark.test.ts:136`.
- `PERF_TRACE` — Toggled to `'1'`/`'0'` around the measured run in `tests/benchmark/profile-deep.test.ts:23`. There are no readers of this variable inside `src/`; it exists as a hook for ad-hoc instrumentation.
## Build & Test Scripts
- `pnpm vitest run` — Run all tests.
- `pnpm vitest convergence` — Run only the convergence suite (also regenerates `ts-output/`).
- `pnpm vitest run -t "example_1"` — Run a single snapshot case by name.
- `BENCH=1 QWIK_SWC_BINDING=/path/to/binding pnpm vitest run tests/benchmark/optimizer-benchmark.test.ts --no-file-parallelism` — Run the SWC-vs-TS benchmarks.
## Platform Requirements
- Node.js >= 20 (>= 20.19 / >= 22.12 in practice, due to oxc NAPI bindings).
- pnpm 10.25 (per `packageManager`).
- A platform supported by oxc NAPI bindings (darwin-arm64, darwin-x64, linux-{arm64,x64,arm,riscv64,...}-{gnu,musl}, win32-x64, win32-arm64, etc. — full matrix in `pnpm-lock.yaml`).
- For benchmarks only: a local checkout of the Qwik monorepo and an absolute path to its native SWC optimizer binding exposed via `QWIK_SWC_BINDING`.
- Not applicable. The package is `private: true` and has no publish/deploy configuration. It is intended to be consumed by Qwik core's existing Vite plugin as a drop-in replacement for the NAPI optimizer; integration steps live outside this repo.
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

## Module System
- `package.json` declares `"type": "module"` and `"engines": { "node": ">=20" }`.
- `tsconfig.json` uses `"module": "NodeNext"` + `"moduleResolution": "NodeNext"` with `"target": "ES2022"` and `"strict": true`.
- All relative imports include the `.js` extension (NodeNext requires it):
- Bare-package imports use no extension:
## File and Directory Naming
- Source files: kebab-case `.ts` — `capture-analysis.ts`, `rewrite-calls.ts`, `transform-session.ts`, `event-capture-promotion.ts`.
- Test files: same kebab-case stem with `.test.ts` suffix — `capture-analysis.test.ts`, `extract.test.ts`.
- The single AST type re-export module is `src/ast-types.ts` (top-level alias hub for `@oxc-project/types`).
- An `index.ts` is used as a barrel only when a directory groups closely related siblings (e.g. `src/optimizer/transform/index.ts`, `src/optimizer/rewrite/index.ts`). Most directories have **no** barrel file; consumers import individual files.
- `src/optimizer/` — main pipeline.
- `src/optimizer/transform/` — phase 4/5 transform pipeline (JSX, segments, diagnostics).
- `src/optimizer/rewrite/` — parent-module rewriting submodules.
- `src/optimizer/segment-codegen/` — segment-module emission helpers.
- `src/optimizer/utils/` — small reusable helpers (parse, AST type-guards, identifier checks, source-loc, qrl naming).
- `src/hashing/` — SipHash-1-3 + symbol naming.
- `src/testing/` — test utilities reused by tests (snapshot parser, ast-compare, batch-runner, metadata-compare).
- `tests/<area>/` mirrors `src/<area>/` (e.g. `tests/optimizer/extract.test.ts` ↔ `src/optimizer/extract.ts`).
## Identifier Naming
- `extractSegments`, `transformModule`, `analyzeCaptures`, `rewriteParentModule`, `buildDisplayName`, `qwikHash`, `parseSnapshot`, `compareAst`, `compareMetadata`.
- `TransformModulesOptions`, `ExtractionResult`, `RewriteContext`, `SegmentAnalysis`, `JsxTransformResult`, `AstCompareResult`.
- AST node aliases use the `Ast` prefix in `src/ast-types.ts`: `AstNode`, `AstProgram`, `AstFunction`, `AstParseResult`, `AstParentNode`, `AstCompatNode`. Treat `Ast*` as the canonical project-internal name; reach for `@oxc-project/types` re-exports only when working with very specific shapes.
- Local variables, function params, fields all camelCase: `bodyText`, `parentScopeIds`, `closureNode`, `qrlVarNames`, `migrationDecisions`.
- Module-level frozen tables / well-known regex objects: SCREAMING_SNAKE_CASE — `RAW_TRANSFER_PARSER_OPTIONS` (`src/ast-types.ts`), `ZERO_KEY` (`src/hashing/siphash.ts`), `DIRECTIVE_MARKER` (`src/optimizer/diagnostics.ts`), `DEFAULT_OPTIONS` (`tests/optimizer/snapshot-options.ts`).
- Internal regex constants (built via `magic-regexp`): camelCase — `leadingSquareBracket`, `trailingSquareBracket`, `paddingParam` (re-exported from `src/optimizer/transform/post-process.ts`).
- QRL variables in emitted code: `q_<symbolName>` (e.g. `q_App_component_HTDRsvUbLiE`). Stripped variants use `q_qrl_<counter>` where `counter = 0xffff0000 + idx*2` (see `preComputeQrlVarNames` in `src/optimizer/rewrite/index.ts`).
- Segment symbol in `prod` mode: `s_<hash>` (see `transformModule` mode-prod block in `src/optimizer/transform/index.ts`).
## TypeScript Style
- Prefer `interface` for object shapes that are part of the public API or appear in many type slots: `TransformModulesOptions`, `TransformOutput`, `ExtractionResult`, `RewriteContext`. See `src/optimizer/types.ts` for the public-API type module.
- Prefer `type` aliases for unions and primitive-ish aliases: `EmitMode = 'dev' | 'prod' | 'lib' | 'hmr'`, `MinifyMode = 'simplify' | 'none'`, `EntryStrategy = { type: 'inline' } | ...`.
- Use `import type { ... }` for type-only imports. Examples in `src/optimizer/extract.ts` lines 12-18.
- Inline `import('...').Type` is used in `src/optimizer/transform/index.ts` for cross-module diagnostic types when adding a top-level import would be redundant.
- Function signatures: parameters typed; return type usually inferred for short helpers, declared explicitly on public/exported functions (`export function transformModule(options: TransformModulesOptions): TransformOutput`).
- Avoid `any`. The project does use a few targeted `any` casts for AST traversal in legacy walks (e.g. `rewriteNoArgMarkers` in `src/optimizer/rewrite/index.ts` accepts `any` for terse recursive walking) — prefer the typed `walk(...)` API from `oxc-walker` for new code instead.
- `isAstNode(value): value is AstCompatNode`
- `isIdentifierNode`, `hasRange`, `isRangedIdentifierNode`
- `isAssignmentPatternNode`, `isPropertyNode`, `isRestElementNode`, `isVariableDeclaratorNode`
- `forEachAstChild(node, visitor, skipKeys?)` — a manual child iterator for cases where the full `oxc-walker` walk is overkill.
## Imports — Order and Grouping
## Regex — magic-regexp, not raw RegExp
## Path Handling — pathe only
## AST Traversal — oxc-walker enter/leave + ScopeTracker
## Text Rewriting — magic-string
- **One `MagicString` per parent module rewrite.** `rewriteParentModule` (`src/optimizer/rewrite/index.ts:168`) constructs `const s = new MagicString(source)` once and threads it through a `RewriteContext`. All sub-passes (`processImports`, `applyModeTransforms`, `rewriteCallSites`, `addCaptureWrapping`, `runJsxTransform`, ...) call `s.overwrite(start, end, text)`, `s.appendLeft`, `s.prependRight`, `s.remove`, then a final `s.toString()` happens in `assembleOutput`.
- **`overwrite(start, end, text)`** is the most common operation — replace an exact source span with new text. Use AST `node.start` / `node.end` byte offsets directly.
- **`appendLeft(pos, text)` vs `prependRight(pos, text)`**: pick based on whether subsequent edits at the same position should appear before or after this one. The codebase uses `appendLeft` for `.w([...])` capture-wrap suffixes and `prependRight` for `/*#__PURE__*/` annotations (see `rewriteCallSites` in `rewrite/index.ts:436`).
- **Wrapped sources for nested transforms.** When a body fragment must be parsed and edited independently (e.g. inlining a $() body), use `createTransformSession` / `createFunctionTransformSession` from `src/optimizer/utils/transform-session.ts`. These wrap the source with a synthetic prefix/suffix, parse, build a `MagicString`, and offer `toSource()` to return only the inner edited text.
- **Never re-print from AST.** No `escodegen` / `astring` / `@babel/generator` is used or allowed. Output preserves original formatting except for explicit `s.overwrite` regions.
## Parsing — Always Through `parseWithRawTransfer`
## Pipeline / Pass Composition
- Phase 0 repair → Phase 1 extract → Phase 2 capture analysis → Phase 3 migration → Phase 4 rewrite parent → Phase 5 generate segments → Phase 6 suppress diagnostics.
## Comments & JSDoc
- **Module-level JSDoc** at the top of every `src/` file with a 1-3 sentence description, e.g. `extract.ts`, `transform/index.ts`, `rewrite/index.ts`. Add this for new modules.
- **Function JSDoc** for exported functions that are not trivially-named getters. Use `@param` / `@returns` only when the name+types are not enough.
- **Inline block comments** mark Rust-correspondence anchors: `// HASH-01:`, `// HASH-02:`, `// MODE-01:`, `// BENCH-01:`. These tag implementation decisions back to a documented requirement; preserve them when refactoring.
- Keep regular code comments brief; explain "why" not "what".
## Error Handling
- `emitC02(identName, file, isClass)` — captured local function/class.
- `emitC05(calleeName, qrlName, file)` — `foo$` called but `fooQrl` not exported.
- `emitPassiveConflictWarning(eventName, file)` — passive vs preventdefault conflict on same JSX element.
- `src/optimizer/transform/index.ts:186` — skip extraction body if re-parse fails.
- `src/optimizer/segment-codegen.ts:287`, `src/optimizer/transform/module-cleanup.ts:68/120/188`.
## Logging
## Function & Module Sizing
- `src/testing/ast-compare.ts` — 3206 lines (a large catalog of AST normalizations; split is non-trivial because each pass shares helpers).
- `src/optimizer/extract.ts` — 690 lines.
- `src/optimizer/transform/index.ts` — 629 lines.
- `src/optimizer/rewrite/index.ts` — 653 lines.
## Lint / Format Tooling
## Public-API Stability Rule
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

## System Overview
```text
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
- One public function (`transformModule`) — pure, no IO. All file paths are
- ESM-only, NodeNext modules, all internal imports use the `.js` extension.
- The optimizer **never reprints from AST**: it parses with `oxc-parser`,
- Hashing is centrally defined in `src/hashing/siphash.ts` (SipHash-1-3 with
- Per-input control flow is a fixed phase sequence (Phase 0 repair → Phase 1
## Layers
- Purpose: Drop-in NAPI-shaped surface
- Location: `src/optimizer/transform/`
- Contains: `index.ts` (`transformModule`), `segment-generation.ts`,
- Depends on: every other layer
- Used by: external Qwik Vite plugin (and tests in `tests/`)
- Purpose: Identify segments and compute their metadata
- Location: `src/optimizer/*.ts`
- Contains: `extract.ts`, `marker-detection.ts`, `capture-analysis.ts`,
- Depends on: `utils/`, `hashing/`, `ast-types.ts`
- Used by: `transform/` and `rewrite/`
- Purpose: Mutate the parent module text using shared `RewriteContext`
- Location: `src/optimizer/rewrite/`
- Contains: `index.ts` (the 11-phase pipeline), `rewrite-context.ts`,
- Depends on: core layer + `utils/transform-session.ts`
- Used by: `transform/index.ts` (`rewriteParentModule`)
- Purpose: Produce per-segment file text plus `SegmentAnalysis`
- Location: `src/optimizer/segment-codegen.ts`,
- Depends on: core layer, `utils/transform-session.ts`
- Used by: `src/optimizer/transform/segment-generation.ts`
- Purpose: Tiny, dependency-free helpers (parsing wrapper, AST shape guards,
- Location: `src/optimizer/utils/`
- Contains: `parse.ts`, `ast.ts`, `binding-pattern.ts`,
- Depends on: only `ast-types.ts` and external packages
- Used by: every other layer in `src/optimizer/`
- Purpose: Replicate Rust hash algorithm and Qwik symbol naming exactly
- Location: `src/hashing/`
- Contains: `siphash.ts`, `naming.ts`, `siphash13.d.ts` (ambient typings for
- Depends on: `siphash` package, `optimizer/path-utils.ts` (basename only)
- Used by: `extract.ts`, `context-stack.ts`, `key-prefix.ts`
- Purpose: Parse Rust insta `.snap` fixtures and structurally compare output
- Location: `src/testing/`
- Contains: `snapshot-parser.ts`, `ast-compare.ts`, `metadata-compare.ts`,
- Depends on: `oxc-parser`, `fast-deep-equal`
- Used by: `tests/optimizer/convergence.test.ts`,
## Data Flow
### Primary Request Path: `transformModule(options)` per input file
### Secondary Flow — Hashing
### Secondary Flow — Transform Sessions (recent refactor)
- No global mutable state. All per-input state lives in local variables of
- `ExtractionResult` is mutated in place across phases (e.g. `parent`,
- The only environment variable read at runtime is `PERF_TRACE` (used by
## Key Abstractions
- Purpose: One row per detected `$()` boundary; carries everything later
- Pattern: Mutable record passed through every phase
- Purpose: Shared mutable bag for the parent-rewrite pipeline; replaces a
- Pattern: Thread-local context object passed to every internal phase
- Purpose: Parse-once + magic-string edit buffer with optional source
- Pattern: Lightweight object factory (`createTransformSession`,
- Purpose: Stack of naming context entries used to derive `displayName`
- Pattern: Class with `push` / `pop` / `peek` / `pushDefaultExport`,
- Purpose: Public per-segment metadata that ships back to the Vite plugin;
- Pattern: Plain interface; `SegmentMetadataInternal` extends it with
## Entry Points
- Location: `src/optimizer/transform/index.ts:90`
- Triggers: External Qwik Vite plugin call; in this repo, every test file
- Responsibilities: Run the full per-input pipeline and return
## Architectural Constraints
- **API shape is frozen:** `TransformModulesOptions`, `TransformOutput`,
- **Hash stability:** `qwikHash()` and `escapeSymbol()` / `buildDisplayName()`
- **Threading:** Single-threaded JS; no worker threads, no async parsing.
- **Global state:** No module-level mutable state. The only caches are local
- **Path handling:** All paths are normalised to forward-slash via
- **Module resolution:** ESM only (`"type": "module"` in `package.json`),
- **Parser options:** Always parse with
## Anti-Patterns
### Reprinting from AST
### Re-parsing inside loops
### Walking AST nodes with hand-written `walk()` recursion
### Reading `path.basename` / `path.extname` from Node's `path`
### Reading bytes / using `crypto` / a different hash
## Error Handling
- `repairInput()` (`src/optimizer/input-repair.ts`) treats an empty parse
- Per-extraction body parses are wrapped in `try { ... } catch { /* skip */ }`
- `compareAst()` (`src/testing/ast-compare.ts:20-50`) retries with a `.tsx`
- Optimizer rule violations (C02, C05, preventdefault/passive conflict)
- `@qwik-disable-next-line C02` and similar comments suppress diagnostics
## Cross-Cutting Concerns
<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->
## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd-debug` for investigation and bug fixing
- `/gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->



<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->

# Codebase Structure

**Analysis Date:** 2026-05-05

## Directory Layout

```
TS-Optimizer/
├── src/
│   ├── ast-types.ts                    # Centralised oxc/ESTree type re-exports + RAW_TRANSFER_PARSER_OPTIONS
│   ├── hashing/                        # Hash + naming, byte-compatible with Rust DefaultHasher
│   │   ├── siphash.ts
│   │   ├── siphash13.d.ts              # Ambient typings for `siphash/lib/siphash13.js`
│   │   └── naming.ts
│   ├── optimizer/                      # Core pipeline (single phase per top-level file)
│   │   ├── transform/                  # Public entry point + segment generation + JSX/event/post-process passes
│   │   ├── rewrite/                    # Parent-module rewrite engine (RewriteContext-based)
│   │   ├── segment-codegen/            # Per-segment body transforms + import recollection
│   │   ├── utils/                      # Tiny dependency-free helpers (parse, ast, paths, sessions, ...)
│   │   ├── extract.ts
│   │   ├── marker-detection.ts
│   │   ├── context-stack.ts
│   │   ├── capture-analysis.ts
│   │   ├── variable-migration.ts
│   │   ├── loop-hoisting.ts
│   │   ├── signal-analysis.ts
│   │   ├── diagnostics.ts
│   │   ├── input-repair.ts
│   │   ├── const-replacement.ts
│   │   ├── strip-ctx.ts
│   │   ├── strip-exports.ts
│   │   ├── entry-strategy.ts
│   │   ├── inline-strategy.ts
│   │   ├── dev-mode.ts
│   │   ├── key-prefix.ts
│   │   ├── path-utils.ts
│   │   ├── rewrite-calls.ts
│   │   ├── rewrite-imports.ts
│   │   ├── segment-codegen.ts          # Public-ish facade re-exporting body-transforms split modules
│   │   └── types.ts                    # NAPI-shaped public types
│   └── testing/                        # Snapshot fixtures parsing + AST/metadata comparison helpers
│       ├── ast-compare.ts
│       ├── batch-runner.ts
│       ├── metadata-compare.ts
│       └── snapshot-parser.ts
├── tests/                              # vitest suites
│   ├── benchmark/                      # SWC-vs-TS perf gates + deep-profile harness
│   ├── hashing/                        # siphash + naming unit tests
│   ├── optimizer/                      # Unit tests + convergence harness + per-snapshot options
│   └── testing/                        # Tests for src/testing helpers
├── match-these-snaps/                  # 209 Rust SWC golden .snap fixtures (insta format) — source of truth
├── ts-output/                          # Auto-regenerated TS optimizer output for each snapshot (209 files)
├── swc-reference-only/                 # Read-only copy of the Rust optimizer source for reference
│   ├── transform/disable_next_line_directive.rs
│   ├── fixtures/index.qwik.mjs
│   └── *.rs                            # transform.rs, code_move.rs, props_destructuring.rs, etc.
├── .planning/                          # Planning artefacts (codebase/, debug/) — created by GSD workflow
├── .claude/                            # Claude Code config + worktrees
│   └── worktrees/                      # Local Claude work-in-progress trees (perf-optimization, const-idents)
├── .idea/                              # JetBrains IDE config
├── CLAUDE.md                           # Project + stack + GSD workflow guidance for Claude
├── DESIGN.md                           # Two-paragraph north-star: virtual edits, then write via magic-string
├── README.md                           # Test-suite overview (convergence + match-these-snaps)
├── package.json                        # Single workspace, pnpm@10.25.0, ESM, Node >=20
├── pnpm-lock.yaml
├── tsconfig.json                       # ES2022 / NodeNext / strict / jsx:react-jsx
├── vitest.config.ts                    # Picks up tests/**/*.test.ts
└── .gitignore                          # Ignores node_modules/, dist/, rust-impl/target/, .tmpbuild/, .pnpm-store/
```

## Directory Purposes

**`src/`:**
- Purpose: All production source for the optimizer library
- Contains: TypeScript only; no generated files
- Key files: `src/optimizer/transform/index.ts` (public `transformModule`),
  `src/optimizer/types.ts` (public API surface),
  `src/ast-types.ts` (type re-exports + parser options)

**`src/optimizer/`:**
- Purpose: Top-level lives the entire optimizer pipeline. Most files map
  one-to-one to a phase or a self-contained transformation
- Contains: ~22 phase-shaped `*.ts` files plus four subdirectories
- Key files: `extract.ts`, `marker-detection.ts`, `context-stack.ts`,
  `capture-analysis.ts`, `variable-migration.ts`, `signal-analysis.ts`,
  `diagnostics.ts`, `types.ts`, `path-utils.ts`

**`src/optimizer/transform/`:**
- Purpose: Public entry point and the JSX / event-handler / segment-emission
  layer that depends on every other piece
- Contains: `index.ts` (`transformModule`), `segment-generation.ts`,
  `module-cleanup.ts`, `post-process.ts`, `dead-code.ts`,
  `diagnostic-detection.ts`, `bind.ts`, `event-handlers.ts`,
  `event-capture-promotion.ts`, `jsx.ts`, `jsx-children.ts`,
  `jsx-elements-core.ts`, `jsx-props.ts`
- Key files: `transform/index.ts` (628 lines; the pipeline driver),
  `transform/segment-generation.ts` (820 lines),
  `transform/jsx.ts` (558 lines)

**`src/optimizer/rewrite/`:**
- Purpose: Parent-module rewrite engine. Self-contained sub-pipeline driven
  by `RewriteContext`
- Contains: `index.ts` (entry: `rewriteParentModule`),
  `rewrite-context.ts` (the shared state type), `output-assembly.ts`,
  `raw-props.ts`, `inline-body.ts`, `const-propagation.ts`
- Key files: `rewrite/index.ts` (652 lines, 11-phase pipeline),
  `rewrite/output-assembly.ts` (522 lines)

**`src/optimizer/segment-codegen/`:**
- Purpose: Splits of the segment-codegen logic kept out of the
  `segment-codegen.ts` facade
- Contains: `body-transforms.ts` (capture unpacking, signature rewrite,
  raw-props application, dead-const removal, sync$ inlining),
  `import-collection.ts` (post-transform import recollection)
- Key files: `body-transforms.ts` (483 lines)

**`src/optimizer/utils/`:**
- Purpose: Tiny, dependency-free helpers shared across phases
- Contains: `parse.ts` (the only `parseSync` wrapper), `ast.ts`
  (type guards + `forEachAstChild`), `binding-pattern.ts`,
  `identifier-name.ts`, `module-symbols.ts`, `props-field-rewrite.ts`,
  `qrl-naming.ts`, `qwik-packages.ts`, `source-loc.ts`,
  `text-scanning.ts`, `transform-session.ts`
- Key files: `transform-session.ts` (parse-once + magic-string edit buffer
  used by raw-props and body transforms)

**`src/hashing/`:**
- Purpose: Replicate Rust's `DefaultHasher` (SipHash-1-3 with zero keys)
  and Qwik symbol-name construction
- Contains: `siphash.ts` (`qwikHash`), `naming.ts` (`escapeSymbol`,
  `buildDisplayName`, `buildSymbolName`), `siphash13.d.ts` (ambient module
  typings for `siphash/lib/siphash13.js`)

**`src/testing/`:**
- Purpose: Helpers for the convergence test harness
- Contains: `snapshot-parser.ts` (parses Rust insta-format `.snap` files),
  `ast-compare.ts` (oxc-parse + `fast-deep-equal` structural compare with a
  position-stripping pass; the largest file in the repo at 3206 lines),
  `metadata-compare.ts` (per-segment metadata comparator),
  `batch-runner.ts` (paged snapshot runner with optional lock file)

**`tests/`:**
- Purpose: vitest-driven test suite. Mirrors `src/` package layout.
- Contains: `tests/benchmark/`, `tests/hashing/`, `tests/optimizer/`,
  `tests/testing/`
- Key files: `tests/optimizer/convergence.test.ts` (golden snapshot
  comparison), `tests/optimizer/snapshot-options.ts` (per-snapshot option
  overrides; 27936 bytes)

**`tests/benchmark/`:**
- Purpose: SWC-vs-TS regression gates and a deep-profile harness
- Contains: `optimizer-benchmark.test.ts` (BENCH-01 monorepo, BENCH-02
  worst-case single file; relies on `QWIK_SWC_BINDING` env var pointing at
  the native NAPI module), `profile-deep.test.ts` (sets `PERF_TRACE=1` and
  re-runs `transformModule` on a worst-case file)

**`tests/hashing/`:**
- Purpose: Pure unit coverage of hash + naming
- Contains: `siphash.test.ts`, `naming.test.ts`

**`tests/optimizer/`:**
- Purpose: Unit tests per top-level optimizer file plus the convergence
  harness
- Contains: 35 `*.test.ts` files (one per major source file) plus
  `snapshot-options.ts` (table of per-snapshot option overrides for the
  convergence run)

**`tests/testing/`:**
- Purpose: Coverage of `src/testing/*` helpers
- Contains: `ast-compare.test.ts`, `batch-runner.test.ts`,
  `metadata-compare.test.ts`, `snapshot-parser.test.ts`

**`match-these-snaps/`:**
- Purpose: 209 golden snapshots produced by Rust SWC optimizer (Rust
  [insta](https://insta.rs/) format with YAML frontmatter). The convergence
  target.
- Generated: Yes (by upstream Rust test suite; refreshed manually — see
  commit d5ae4a4 "chore: refresh upstream optimizer snapshots and
  references")
- Committed: Yes
- Naming: `qwik_core__test__<test_name>.snap` (e.g.
  `qwik_core__test__example_1.snap`,
  `qwik_core__test__component_level_self_referential_qrl.snap`)
- Format per file: YAML frontmatter, `==INPUT==` section, one or more
  `============================= filename ==` section delimiters (with
  optional `(ENTRY POINT)` marker), per-segment metadata in `/* { ... } */`
  block comments, optional `Some("<source-map>")` line, trailing
  `== DIAGNOSTICS ==` JSON array

**`ts-output/`:**
- Purpose: Mirror of `match-these-snaps/` content in the same insta-style
  format, regenerated every time `tests/optimizer/convergence.test.ts`
  runs. Lets diffs against `match-these-snaps/` answer "did my change
  improve or regress convergence?"
- Generated: Yes (auto-written by the convergence test —
  `tests/optimizer/convergence.test.ts:100`,
  `formatSnapshot()` at `:30-45`)
- Committed: Yes (so PRs show snapshot drift)

**`swc-reference-only/`:**
- Purpose: Read-only copy of the Rust optimizer crate's source files so
  contributors can cross-reference behaviour without leaving the repo.
  Never imported, never compiled.
- Contains: `transform.rs`, `code_move.rs`, `collector.rs`, `errors.rs`,
  `entry_strategy.rs`, `props_destructuring.rs`, `rename_imports.rs`,
  `inlined_fn.rs`, `const_replace.rs`, `is_const.rs`, `parse.rs`,
  `lib.rs`, `test.rs`, `add_side_effect.rs`, `clean_side_effects.rs`,
  `dependency_analysis.rs`, `filter_exports.rs`, `utils.rs`, `words.rs`,
  `transform/disable_next_line_directive.rs`, `fixtures/index.qwik.mjs`
- Generated: No
- Committed: Yes

**`.planning/`:**
- Purpose: GSD workflow planning artefacts; not consumed at runtime
- Contains: `codebase/` (these documents), `debug/` (debug session notes)
- Generated: Partially (this directory)
- Committed: No (currently untracked per `git status`)

**`.claude/`:**
- Purpose: Claude Code per-project config + git worktrees for parallel
  Claude work
- Contains: `worktrees/perf-optimization/`, `worktrees/const-idents/`
- Committed: Project config files only; worktree contents are scoped local

## Key File Locations

**Public Entry Point (single):**
- `src/optimizer/transform/index.ts` — `transformModule(options)`,
  the only exported pipeline

**Public Type Surface:**
- `src/optimizer/types.ts` — `TransformModulesOptions`, `TransformOutput`,
  `TransformModule`, `SegmentAnalysis`, `Diagnostic`, `EntryStrategy`,
  `MinifyMode`, `EmitMode`. Must match the Rust NAPI shape.
- `src/ast-types.ts` — Re-exports the oxc/ESTree node union types and
  exposes `RAW_TRANSFER_PARSER_OPTIONS`.

**Configuration:**
- `package.json` — `"type": "module"`, `engines.node >= 20`,
  `packageManager: pnpm@10.25.0`
- `tsconfig.json` — `target: ES2022`, `module: NodeNext`,
  `moduleResolution: NodeNext`, `jsx: react-jsx`, `strict: true`
- `vitest.config.ts` — `include: ['tests/**/*.test.ts']`,
  `globals: false`
- `.gitignore` — Ignores `node_modules/`, `dist/`, `rust-impl/target/`,
  `.tmpbuild/`, `.pnpm-store/`, `isolate-*.log`, `.logs/subtask2.log`

**Core Logic (per pipeline phase):**
- Repair: `src/optimizer/input-repair.ts`
- Parse: `src/optimizer/utils/parse.ts`
- Marker detection: `src/optimizer/marker-detection.ts`
- Extraction: `src/optimizer/extract.ts`
- Naming/hash: `src/optimizer/context-stack.ts`,
  `src/hashing/naming.ts`, `src/hashing/siphash.ts`
- Capture analysis: `src/optimizer/capture-analysis.ts`
- Migration: `src/optimizer/variable-migration.ts`
- Loop hoisting + event capture promotion:
  `src/optimizer/loop-hoisting.ts`,
  `src/optimizer/transform/event-capture-promotion.ts`
- Diagnostics: `src/optimizer/diagnostics.ts`,
  `src/optimizer/transform/diagnostic-detection.ts`
- Parent rewrite: `src/optimizer/rewrite/index.ts` (orchestrator),
  `src/optimizer/rewrite/rewrite-context.ts`,
  `src/optimizer/rewrite/output-assembly.ts`,
  `src/optimizer/rewrite/raw-props.ts`,
  `src/optimizer/rewrite/inline-body.ts`,
  `src/optimizer/rewrite/const-propagation.ts`
- JSX transform: `src/optimizer/transform/jsx.ts` (+ `jsx-*.ts` siblings)
- Signals: `src/optimizer/signal-analysis.ts`
- Segment codegen: `src/optimizer/transform/segment-generation.ts`,
  `src/optimizer/segment-codegen.ts`,
  `src/optimizer/segment-codegen/body-transforms.ts`,
  `src/optimizer/segment-codegen/import-collection.ts`
- Post-process: `src/optimizer/transform/post-process.ts`,
  `src/optimizer/transform/module-cleanup.ts`,
  `src/optimizer/transform/dead-code.ts`
- Mode/strategy helpers: `src/optimizer/strip-ctx.ts`,
  `src/optimizer/strip-exports.ts`,
  `src/optimizer/inline-strategy.ts`,
  `src/optimizer/entry-strategy.ts`,
  `src/optimizer/dev-mode.ts`,
  `src/optimizer/const-replacement.ts`,
  `src/optimizer/key-prefix.ts`

**Transform sessions (recent):**
- `src/optimizer/utils/transform-session.ts` — parse-once + MagicString
  edit buffer; backs `raw-props.ts`, `body-transforms.ts`. Introduced in
  commit 85a9059 ("refactor: move optimizer rewrites onto transform
  sessions"); replaces brittle text rewrites in raw-props handling.

**Testing:**
- Unit tests: `tests/optimizer/<module>.test.ts` (mirror file naming with
  source)
- Convergence: `tests/optimizer/convergence.test.ts`
- Per-snapshot options: `tests/optimizer/snapshot-options.ts`
- Snapshot fixtures: `match-these-snaps/qwik_core__test__*.snap`
- Helpers under test: `tests/testing/*.test.ts` exercise
  `src/testing/*.ts`
- Hashing: `tests/hashing/siphash.test.ts`,
  `tests/hashing/naming.test.ts`
- Benchmarks: `tests/benchmark/optimizer-benchmark.test.ts` (BENCH-01,
  BENCH-02, gated on `QWIK_SWC_BINDING` env var),
  `tests/benchmark/profile-deep.test.ts`

## Naming Conventions

**Files:**
- Pattern: `kebab-case.ts`. Examples: `marker-detection.ts`,
  `capture-analysis.ts`, `event-capture-promotion.ts`,
  `transform-session.ts`.
- Tests mirror source basename: `src/optimizer/extract.ts` →
  `tests/optimizer/extract.test.ts`. Module-spanning tests use a
  short suffix: `tests/optimizer/jsx-transform.test.ts`,
  `tests/optimizer/event-handler-transform.test.ts`.
- Snapshot fixtures: `qwik_core__test__<rust_test_name>.snap` (matches
  the Rust insta default; never rename).
- Re-export entry points are named `index.ts`
  (`src/optimizer/transform/index.ts`, `src/optimizer/rewrite/index.ts`).
- Type-only modules use `*-types.ts` (`src/ast-types.ts`).
- Ambient declaration: `*.d.ts` (`src/hashing/siphash13.d.ts`).

**Directories:**
- Pattern: lowercase, hyphenated when multi-word. Examples: `optimizer`,
  `segment-codegen`, `match-these-snaps`, `ts-output`,
  `swc-reference-only`.

**Functions & types:**
- Functions: `camelCase` verbs — `transformModule`, `extractSegments`,
  `analyzeCaptures`, `buildDisplayName`, `qwikHash`, `parseWithRawTransfer`.
- Types/interfaces: `PascalCase` —
  `TransformModulesOptions`, `ExtractionResult`, `RewriteContext`,
  `SegmentMetadata`, `SegmentAnalysis`.
- AST type aliases use the `Ast` prefix (`AstNode`, `AstProgram`,
  `AstFunction`, `AstCompatNode`).
- Public NAPI types do not carry the `Ast` prefix.

**Modules vs functions:**
- One pipeline phase per file at the top level of `src/optimizer/`.
- Subdirectories collect closely-coupled splits of a single concept
  (e.g. `src/optimizer/rewrite/` is the rewrite engine; the `index.ts`
  re-exports the per-piece modules).

## Where to Add New Code

**A new optimizer phase / pass:**
- Decide whether it mutates the parent module (→ `src/optimizer/rewrite/`),
  generates segment text (→ `src/optimizer/segment-codegen/` or
  `src/optimizer/transform/segment-generation.ts`), or precomputes
  metadata (→ a new `src/optimizer/<phase-name>.ts` file).
- Wire it into the per-input loop in
  `src/optimizer/transform/index.ts`. Keep phase ordering documented at
  the file header.
- Co-locate unit tests as `tests/optimizer/<phase-name>.test.ts`.

**A new helper used by multiple phases:**
- Place in `src/optimizer/utils/` if it has zero pipeline awareness (e.g.
  `binding-pattern.ts`, `text-scanning.ts`).
- Otherwise put it in the phase folder closest to its caller.

**A new public API option (must mirror Rust):**
- Add to `TransformModulesOptions` in `src/optimizer/types.ts`.
- Confirm the matching field exists in the Rust NAPI module — cross-check
  against `swc-reference-only/lib.rs` and `swc-reference-only/transform.rs`.
- Thread through `transformModule` in
  `src/optimizer/transform/index.ts`; if it changes parent rewriting, also
  thread through `rewriteParentModule` and add a field on `RewriteContext`
  in `src/optimizer/rewrite/rewrite-context.ts`.

**A new diagnostic:**
- Add the emitter to `src/optimizer/diagnostics.ts` (next to
  `emitC02` / `emitC05` / `emitPassiveConflictWarning`).
- Add detection logic in
  `src/optimizer/transform/diagnostic-detection.ts` and call it from
  `src/optimizer/transform/index.ts` (Phase 2c or Phase 4c slot).
- Make sure suppression directive parsing in
  `parseDisableDirectives()` already covers the new diagnostic code.

**A new AST transform that needs to edit text and re-parse the result:**
- Use `createTransformSession` /
  `createFunctionTransformSession` from
  `src/optimizer/utils/transform-session.ts`. Do **not** call `parseSync`
  twice on the same body — that pattern is the anti-pattern fixed in
  commit 85a9059.

**A new test fixture covering Rust SWC behaviour:**
- Drop the `.snap` file into `match-these-snaps/` (only when
  refreshing from Rust upstream — see
  `swc-reference-only/test.rs` for the canonical names).
- If the fixture needs non-default options, add an entry to
  `tests/optimizer/snapshot-options.ts`.
- Run the convergence test; `ts-output/` will be populated automatically.

**A new unit test:**
- Add `tests/<area>/<source-basename>.test.ts` next to the existing
  pattern. Use vitest's `describe`/`it`/`expect` from `'vitest'`. No
  globals (`vitest.config.ts` sets `globals: false`).

**A benchmark:**
- Place under `tests/benchmark/`. Wrap SWC-dependent assertions in a
  `QWIK_SWC_BINDING` env-var guard so the suite is skipped when the
  binding is unavailable (current pattern at
  `tests/benchmark/optimizer-benchmark.test.ts:42-45`).

## Special Directories

**`match-these-snaps/`:**
- Purpose: Convergence target (Rust SWC golden output for 209 fixtures)
- Generated: Yes, by Rust upstream
- Committed: Yes (refreshed manually)

**`ts-output/`:**
- Purpose: Auto-regenerated mirror of `match-these-snaps/` containing the
  current TS optimizer output. Used as a diff baseline.
- Generated: Yes (overwritten by `tests/optimizer/convergence.test.ts`)
- Committed: Yes (so snapshot drift surfaces in PR diffs)

**`swc-reference-only/`:**
- Purpose: Read-only Rust optimizer source for cross-reference. Never
  imported, never compiled.
- Generated: No
- Committed: Yes

**`.planning/`:**
- Purpose: GSD workflow planning artefacts
- Generated: By GSD commands
- Committed: No (`.planning/` is untracked)

**`.claude/worktrees/`:**
- Purpose: Local Claude Code parallel worktrees (`perf-optimization`,
  `const-idents`)
- Generated: Yes
- Committed: No

**`.idea/`:**
- Purpose: JetBrains IDE settings
- Generated: Yes
- Committed: Yes (per current `git ls-files`)

---

*Structure analysis: 2026-05-05*

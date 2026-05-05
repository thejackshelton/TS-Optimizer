# External Integrations

**Analysis Date:** 2026-05-05

## Summary

This project is a pure in-process TypeScript library — a drop-in replacement for Qwik's Rust/SWC optimizer NAPI module. It performs **no** HTTP I/O, **no** database access, **no** authentication, and **no** outbound network calls at runtime. All "integrations" are either:

1. Native NAPI bindings consumed in-process (oxc-parser, oxc-transform).
2. Pure-JS algorithmic libraries (siphash, magic-string, magic-regexp, pathe, fast-deep-equal).
3. Compatibility contracts with the upstream Qwik project (NAPI surface this library replaces, and the SWC golden snapshots used as the convergence target).

There are explicitly **no traditional external integrations** (no APIs, no databases, no auth, no telemetry, no CI/CD pipelines configured in-repo).

## Native NAPI Bindings

These are Rust/native libraries loaded via N-API at runtime. They are listed under `devDependencies` in `package.json` but are imported directly by source code in `src/` and run on every transform call.

**`oxc-parser` ^0.124.0** — TypeScript/TSX/JSX parser.
- Sole entry point: `src/optimizer/utils/parse.ts` calls `parseSync(filename, sourceText, RAW_TRANSFER_PARSER_OPTIONS)`.
- Raw-transfer mode (`experimentalRawTransfer: true`) is configured in `src/ast-types.ts` to avoid the JS↔native serialization cost on hot paths.
- Also called directly by the test harness in `src/testing/ast-compare.ts` and several test files for re-parsing expected/actual output.
- Platform bindings shipped per-arch via `@oxc-parser/binding-*` packages (resolved transitively in `pnpm-lock.yaml`).

**`oxc-transform` ^0.124.0** — TypeScript syntax stripper.
- Called as `transformSync(...)` from:
  - `src/optimizer/rewrite/output-assembly.ts` (parent module TS strip during final assembly)
  - `src/optimizer/transform/post-process.ts` (segment-level TS strip + JSX downlevel decisions)
  - `src/optimizer/transform/module-cleanup.ts` (post-DCE cleanup)
- Drives the `transpileTs` / `transpileJsx` options exposed by `transformModule()`.

**`oxc-walker` ^0.7.0** — Pure-JS AST walker (wraps `estree-walker`) with `ScopeTracker`. Not native, but listed here for symmetry with the oxc family.
- Used as `walk(...)` and `getUndeclaredIdentifiersInFunction(...)` in:
  - `src/optimizer/extract.ts`
  - `src/optimizer/capture-analysis.ts`
  - `src/optimizer/const-replacement.ts`
  - `src/optimizer/variable-migration.ts`
  - `src/optimizer/segment-codegen/import-collection.ts`
  - `src/optimizer/segment-codegen/body-transforms.ts`
  - `src/optimizer/transform/event-capture-promotion.ts`
  - `src/optimizer/transform/diagnostic-detection.ts`
  - `src/optimizer/transform/module-cleanup.ts`
  - `src/optimizer/transform/jsx.ts`
  - `src/optimizer/transform/segment-generation.ts`
  - Plus tests: `tests/optimizer/loop-hoisting.test.ts`, `tests/optimizer/capture-analysis.test.ts`.
- Coupled to `@oxc-project/types` 0.124.0 (pinned) for AST node shapes — re-exported from `src/ast-types.ts`.

## Compatibility-Critical Algorithmic Libraries

**`siphash` ^1.1.0 (resolved 1.2.0)** — SipHash-1-3 hash function.
- Imported via subpath: `import SipHash13 from 'siphash/lib/siphash13.js'` in `src/hashing/siphash.ts:9`.
- Local ambient module declaration in `src/hashing/siphash13.d.ts` types the subpath import.
- **Compatibility constraint:** must produce byte-identical output to Rust's `std::collections::hash_map::DefaultHasher` with zero keys (`[0,0,0,0]`). The hash is the entire identity of a Qwik segment — every QRL reference in generated code, every snapshot file name, and every runtime lookup depends on byte-for-byte equality with the upstream Rust optimizer. Verified by `tests/hashing/siphash.test.ts` against fixed input/output pairs.
- Encoding pipeline (`qwikHash` in `src/hashing/siphash.ts:21`): SipHash-1-3 → u64 little-endian → base64url no-pad → replace `-`/`_` with `0`. Returns 11-character symbol.

**`magic-string` ^0.30.21** — Source-text editor that preserves original text and supports source maps.
- The single output-assembly mechanism for the entire optimizer. Per `DESIGN.md`: hold operations in a virtual representation, then materialize once with magic-string.
- Default export imported by all rewrite/codegen modules; canonical owner of the per-file edit session is `src/optimizer/utils/transform-session.ts` (the `MagicString` instance lives on the transform session and is threaded through rewrite phases).
- Other consumers: `src/optimizer/rewrite/index.ts`, `src/optimizer/rewrite/inline-body.ts`, `src/optimizer/rewrite/rewrite-context.ts`, `src/optimizer/segment-codegen.ts`, `src/optimizer/transform/jsx.ts`, `src/optimizer/transform/jsx-elements-core.ts`, `src/optimizer/transform/module-cleanup.ts`, `src/optimizer/strip-exports.ts`, `src/optimizer/const-replacement.ts`.

**`magic-regexp` ^0.11.0** — Type-safe regex builder.
- Used wherever post-AST text-level patterns are needed (TS-strip probes, JSX child whitespace handling, Qwik-specific markers, base64 cleanup in the hash, etc.).
- Hottest consumers: `src/optimizer/segment-codegen/body-transforms.ts`, `src/optimizer/transform/post-process.ts`, `src/optimizer/transform/dead-code.ts`, `src/optimizer/transform/module-cleanup.ts`, `src/hashing/siphash.ts`, `src/testing/snapshot-parser.ts`.

**`pathe` ^2.0.3** — Forward-slash path normalization.
- Sole entry: `src/optimizer/path-utils.ts` (`basename`, `dirname`, `extname`, `normalize`, `normalizeString`, `relative`).
- Compatibility-critical because the relative path is part of the hash input — different separators on Windows vs POSIX would produce different segment hashes. Mirrors Rust's `to_slash_lossy()`.

**`fast-deep-equal` ^3.1.3** — Test-only structural equality.
- Used in `src/testing/ast-compare.ts` to compare cleaned AST trees during convergence testing, plus in `tests/optimizer/failure-families.test.ts` and `tests/optimizer/convergence-breakdown.test.ts`.

## Compatibility Contract: Qwik NAPI Surface (Replaced)

This library is the JavaScript/TypeScript reimplementation of the optimizer that Qwik core's Vite plugin invokes. The contract is the NAPI module's `transform_modules` function.

**TS-side public API (the replacement surface):**
- `transformModule(options: TransformModulesOptions): TransformOutput` exported from `src/optimizer/transform/index.ts:90`.
- Input/output types defined in `src/optimizer/types.ts` (`TransformModulesOptions`, `TransformModuleInput`, `TransformOutput`, `TransformModule`, `SegmentAnalysis`, `Diagnostic`, `EntryStrategy`, `EmitMode`, `MinifyMode`).
- Per the file header, "These types must match the NAPI binding interface exactly so the TypeScript optimizer is a drop-in replacement for the SWC optimizer."

**Native side (the thing being replaced, not depended upon):**
- The upstream Qwik SWC optimizer NAPI binding is **not** declared in `package.json`. It is referenced only at benchmark time via the `QWIK_SWC_BINDING` environment variable.
- Loaded via `createRequire(import.meta.url)` + `require(QWIK_SWC_BINDING)` at `tests/benchmark/optimizer-benchmark.test.ts:50`. The path must point at a Qwik native binding `.node` file (or its loader) on the developer's machine.
- Invoked as `swcBinding.transform_modules(swcOptions)` (async) — `tests/benchmark/optimizer-benchmark.test.ts:150`, `:167`, `:227`, `:244`. The TS implementation's `transformModule(...)` is the synchronous equivalent and the benchmarks compare the two head-to-head.

**Qwik package identifiers recognized at transform time:**
- `@qwik.dev/core`, `@qwik.dev/core/build`, `@qwik.dev/react`, `@qwik.dev/router`
- `@builder.io/qwik`, `@builder.io/qwik/build`, `@builder.io/qwik-city`, `@builder.io/qwik-city/build`, `@builder.io/qwik-react`
- Centralized in `src/optimizer/utils/qwik-packages.ts` (`isQwikPackageSource`, `QWIK_PACKAGE_PREFIXES`).
- Legacy `@builder.io/*` → `@qwik.dev/*` rewrite map in `src/optimizer/rewrite-imports.ts:12-14`.
- Const-import sources for `_const_replace` semantics in `src/optimizer/const-replacement.ts:19-23` and `src/optimizer/transform/module-cleanup.ts:43-49`.
- These strings are not network/HTTP integrations — they are import-source identifiers the optimizer recognizes when processing user code.

## Test Fixtures Pulled from Upstream Qwik

**`match-these-snaps/`** — 210 files (209 `.snap` files plus a sentinel). These are golden snapshots produced by the Rust SWC optimizer in Rust's [insta](https://insta.rs/) format with YAML frontmatter (e.g. `qwik_core__test__example_1.snap`).
- Source: `packages/optimizer/core/src/test.rs` in QwikDev/qwik (per the `source:` field in each snapshot's YAML frontmatter, e.g. `source: packages/optimizer/core/src/test.rs`).
- Treated as immutable. They define the convergence target.
- Consumed by `tests/optimizer/convergence.test.ts`, `tests/optimizer/convergence-breakdown.test.ts`, `tests/optimizer/snapshot-batch.test.ts`, and the parser in `src/testing/snapshot-parser.ts`.

**`ts-output/`** — 211 files. Regenerated on every `convergence` test run (208 of 209 snapshots have an `==INPUT==` section to transform; the rest are skipped). Not a vendored fixture set — derived output of the current TS implementation.

**`swc-reference-only/`** — Vendored Rust source from the upstream Qwik optimizer (`transform.rs`, `collector.rs`, `code_move.rs`, `entry_strategy.rs`, `errors.rs`, `filter_exports.rs`, `inlined_fn.rs`, `parse.rs`, `is_const.rs`, `props_destructuring.rs`, `rename_imports.rs`, `add_side_effect.rs`, `clean_side_effects.rs`, `const_replace.rs`, `dependency_analysis.rs`, `lib.rs`, `test.rs`, `utils.rs`, `words.rs`, plus `transform/` and `fixtures/` subtrees). Read-only reference material for porting; **not compiled, not executed, not imported** by any TypeScript code.

**External Qwik checkout (developer-local, not vendored)** — Hardcoded path `/Users/jackshelton/dev/open-source/qwik/packages` in `tests/benchmark/optimizer-benchmark.test.ts:33` and `tests/benchmark/profile-deep.test.ts:6`. Required only to run benchmarks.

## Data Storage

- **Databases:** None.
- **File storage:** Local filesystem only — reads `match-these-snaps/*.snap`, writes `ts-output/*.snap` during convergence runs (`tests/optimizer/convergence.test.ts`, `src/testing/batch-runner.ts`).
- **Caching:** None (process is run-and-exit; an in-memory `discoveredFiles` cache exists in `tests/benchmark/optimizer-benchmark.test.ts:56` for repeated `find`s within a benchmark run).

## Authentication & Identity

Not applicable. No identity, secrets, or credentials are read or stored. No `.env` files exist in the repo.

## Monitoring & Observability

- **Error tracking:** None.
- **Logs:** None in `src/`. Benchmark tests use `console.log` to print human-readable timing tables (`tests/benchmark/optimizer-benchmark.test.ts:191-201`, `:268-279`).
- **Tracing hook:** `process.env.PERF_TRACE` is toggled around the measured run in `tests/benchmark/profile-deep.test.ts:23-25`. No reader for this variable currently exists in `src/`; it is a vestigial / instrumentation hook.

## CI/CD & Deployment

- **CI pipeline:** None configured in-repo (no `.github/workflows/`, no `.gitlab-ci.yml`, no `.circleci/`).
- **Hosting / deployment:** Not applicable. `package.json` has `"private": true` and no publish scripts. Distribution model is "consumed via direct import" — see CLAUDE.md ("Consumed as a library function by Qwik core's existing Vite plugin").

## Webhooks & Callbacks

- **Incoming:** None.
- **Outgoing:** None.

## Environment Configuration

**Variables read at runtime (test/benchmark only):**
- `QWIK_SWC_BINDING` — Absolute path to the upstream Qwik SWC NAPI binding for benchmarks. Read in `tests/benchmark/optimizer-benchmark.test.ts:42`.
- `BENCH` — Set to `1` to enable benchmark suites. Read in `tests/benchmark/optimizer-benchmark.test.ts:136`.
- `PERF_TRACE` — Toggled in `tests/benchmark/profile-deep.test.ts:23`; not currently consumed.

**Secrets location:** None. No secrets, no credentials, no API keys are required to develop, test, or run this project. The `QWIK_SWC_BINDING` value is a filesystem path, not a credential.

---

*Integration audit: 2026-05-05*

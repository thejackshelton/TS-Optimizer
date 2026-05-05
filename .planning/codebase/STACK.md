# Technology Stack

**Analysis Date:** 2026-05-05

## Languages

**Primary:**
- TypeScript ~5.7 (resolved 5.9.3) — All source under `src/` and tests under `tests/`. Strict mode enabled in `tsconfig.json`.

**Secondary:**
- JSX/TSX — Parsed and emitted by the optimizer; `tsconfig.json` sets `"jsx": "react-jsx"` for any in-repo TSX.
- Rust (reference only) — `swc-reference-only/` contains the upstream SWC optimizer source as a read-only reference. NOT compiled or executed by this project.

## Runtime

**Environment:**
- Node.js >= 20 LTS (declared in `package.json` `engines.node`).
- ESM-only (`"type": "module"` in `package.json`; `"module": "NodeNext"` in `tsconfig.json`).
- Native NAPI bindings (oxc-parser, oxc-transform) require Node `^20.19.0 || >=22.12.0` per oxc binding manifests.

**Package Manager:**
- pnpm 10.25.0 (declared via `"packageManager"` field in `package.json`).
- Lockfile: present (`pnpm-lock.yaml`, lockfileVersion 9.0).

## Frameworks

**Core:**
- None — this is a library. No web/server framework.

**Testing:**
- vitest ^4.1.4 (resolved 4.1.4) — Test runner, assertions, and coverage. Config: `vitest.config.ts`. Picks up `tests/**/*.test.ts`, runs with `globals: false` (explicit imports only).

**Build/Dev:**
- TypeScript compiler 5.9.3 — Type checking only. There is currently no build script. `tsconfig.json` declares `outDir: "dist"` with `declaration`, `declarationMap`, and `sourceMap` enabled, but `dist/` is gitignored and `package.json` defines no `build` / `prepublish` scripts. The package is `private: true` and consumed in-repo only.
- oxc-transform ^0.124.0 — Used at runtime by the optimizer itself for TS-stripping the generated parent/segment modules. Also serves as the only "downlevel" tool in the project; no Babel, no tsc emit, no esbuild.

## Key Dependencies

**Critical (runtime — `dependencies`):**
- `magic-string` ^0.30.21 — Surgical source text mutation. Drives the entire output assembly pipeline (`src/optimizer/utils/transform-session.ts`, `src/optimizer/rewrite/index.ts`, `src/optimizer/segment-codegen.ts`, etc.). Avoids re-printing from AST.
- `magic-regexp` ^0.11.0 — Type-safe regex builder. Used pervasively for tokenizing and rewriting source text where AST traversal would be overkill. Hot paths in `src/optimizer/segment-codegen/body-transforms.ts`, `src/optimizer/transform/post-process.ts`, `src/optimizer/transform/dead-code.ts`, `src/hashing/siphash.ts`.
- `pathe` ^2.0.3 — Cross-platform path normalization (forward-slash). Used in `src/optimizer/path-utils.ts` to match the Rust optimizer's `to_slash_lossy()` semantics; affects hash inputs so platform-stable behavior is required.
- `siphash` ^1.1.0 (resolved 1.2.0) — SipHash-1-3 implementation. Compatibility-critical: must produce byte-identical hashes to Rust's `std::collections::hash_map::DefaultHasher` (zero keys). Imported via subpath `siphash/lib/siphash13.js` in `src/hashing/siphash.ts`. Local ambient declaration: `src/hashing/siphash13.d.ts`.

**Critical (parse/transform — `devDependencies`, but used at runtime by tests and by the optimizer itself):**
- `oxc-parser` ^0.124.0 — ESTree-conformant TS/TSX/JSX parser via Rust NAPI. Sole entry: `src/optimizer/utils/parse.ts` calls `parseSync(filename, sourceText, RAW_TRANSFER_PARSER_OPTIONS)`. Raw-transfer mode (`experimentalRawTransfer: true`) is enabled in `src/ast-types.ts`.
- `oxc-transform` ^0.124.0 — TS syntax stripping via NAPI. Used in `src/optimizer/rewrite/output-assembly.ts`, `src/optimizer/transform/post-process.ts`, and `src/optimizer/transform/module-cleanup.ts` (all imported as `transformSync as oxcTransformSync`).
- `oxc-walker` ^0.7.0 — AST traversal with built-in scope tracking. Imported as `walk` and `getUndeclaredIdentifiersInFunction` across the optimizer (capture analysis, JSX rewrite, segment codegen, diagnostic detection).
- `@oxc-project/types` 0.124.0 (pinned, no caret) — ESTree node type definitions for oxc. Re-exported from `src/ast-types.ts`. Pinned to match `oxc-parser`'s emitted shapes exactly.

**Test-only:**
- `fast-deep-equal` ^3.1.3 — Structural AST equality for convergence comparisons. Used by `src/testing/ast-compare.ts`, `tests/optimizer/failure-families.test.ts`, `tests/optimizer/convergence-breakdown.test.ts`.

**Type-only:**
- `@types/node` ^25.6.0 — Node.js type definitions for `node:fs`, `node:path`, `node:module`, `node:url`, `node:child_process` used in tests and the batch runner.

## Configuration

**TypeScript (`tsconfig.json`):**
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

**Vitest (`vitest.config.ts`):**
- `test.include`: `tests/**/*.test.ts`
- `test.globals`: `false` (suite primitives must be imported)
- No coverage thresholds configured.
- No setup files.

**Package (`package.json`):**
- `"name": "qwik-optimizer-ts"`, `"version": "0.0.1"`, `"private": true`
- `"type": "module"`, `"engines": { "node": ">=20" }`
- No `main` / `exports` / `bin` / `types` entries — not published; consumed via direct `import` from `src/optimizer/transform/index.js` in tests/benchmarks.

**Environment Variables:**
- `QWIK_SWC_BINDING` — Absolute path to the upstream Qwik native NAPI optimizer binding (`.node` file or its loader). Read in `tests/benchmark/optimizer-benchmark.test.ts` to enable the SWC vs TS comparison benchmarks. When unset/empty, the benchmark suite is registered as the skipped placeholder `BENCH-00: QWIK_SWC_BINDING was not set, skipping benchmark tests.`
- `BENCH` — Set to `1` to run the benchmark suites (`BENCH-01` full monorepo, `BENCH-02` worst-case single file). Otherwise both are wrapped in `describe.skip`. See `tests/benchmark/optimizer-benchmark.test.ts:136`.
- `PERF_TRACE` — Toggled to `'1'`/`'0'` around the measured run in `tests/benchmark/profile-deep.test.ts:23`. There are no readers of this variable inside `src/`; it exists as a hook for ad-hoc instrumentation.

**No `.env` files** are present in the repo, and none are referenced by source.

## Build & Test Scripts

From `package.json`:

```json
"scripts": {
  "test": "vitest run",
  "test:watch": "vitest"
}
```

Common invocations (per `README.md`):
- `pnpm vitest run` — Run all tests.
- `pnpm vitest convergence` — Run only the convergence suite (also regenerates `ts-output/`).
- `pnpm vitest run -t "example_1"` — Run a single snapshot case by name.
- `BENCH=1 QWIK_SWC_BINDING=/path/to/binding pnpm vitest run tests/benchmark/optimizer-benchmark.test.ts --no-file-parallelism` — Run the SWC-vs-TS benchmarks.

There is no `build`, `lint`, `format`, or `typecheck` script. Type checking happens implicitly via the editor / `tsc --noEmit` if invoked manually.

## Platform Requirements

**Development:**
- Node.js >= 20 (>= 20.19 / >= 22.12 in practice, due to oxc NAPI bindings).
- pnpm 10.25 (per `packageManager`).
- A platform supported by oxc NAPI bindings (darwin-arm64, darwin-x64, linux-{arm64,x64,arm,riscv64,...}-{gnu,musl}, win32-x64, win32-arm64, etc. — full matrix in `pnpm-lock.yaml`).
- For benchmarks only: a local checkout of the Qwik monorepo and an absolute path to its native SWC optimizer binding exposed via `QWIK_SWC_BINDING`.

**Production:**
- Not applicable. The package is `private: true` and has no publish/deploy configuration. It is intended to be consumed by Qwik core's existing Vite plugin as a drop-in replacement for the NAPI optimizer; integration steps live outside this repo.

---

*Stack analysis: 2026-05-05*

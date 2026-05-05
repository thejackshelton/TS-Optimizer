# Testing Patterns

**Analysis Date:** 2026-05-05

This document describes how the optimizer is tested in practice — frameworks, fixture sources, comparison strategy, and how to add new tests so they fit the existing patterns.

## Test Framework

**Runner:** Vitest 4.1.4 (`vitest` in `devDependencies`).

- Config: `vitest.config.ts` (8 lines).
  ```ts
  import { defineConfig } from 'vitest/config';

  export default defineConfig({
    test: {
      include: ['tests/**/*.test.ts'],
      globals: false,
    },
  });
  ```
- `globals: false` — tests must explicitly `import { describe, it, expect } from 'vitest';`. No ambient globals.
- Vitest-config-implicit defaults: file-parallel, all `*.test.ts` under `tests/`, vitest's built-in TypeScript transform (no separate ts-node / babel step).

**Assertions / matchers:** Vitest's built-in `expect` (`.toBe`, `.toEqual`, `.toHaveLength`, `.toBeNull`, `.toBeDefined`, `.toContain`, `.toMatch`, `.toBeGreaterThan`, `.toBeLessThanOrEqual`, etc.).

**Run commands** (see `package.json`):
```bash
pnpm test              # vitest run            -- single CI-style pass
pnpm test:watch        # vitest                -- watch mode

# Run a single suite by file
pnpm vitest run tests/optimizer/extract.test.ts

# Run a single test by name pattern
pnpm vitest run -t "example_1"

# Run convergence (also regenerates ts-output/)
pnpm vitest convergence

# Run benchmarks (gated on env, see below)
BENCH=1 QWIK_SWC_BINDING=/path/to/swc/binding.node \
  pnpm vitest run tests/benchmark/optimizer-benchmark.test.ts --no-file-parallelism
```

## Test File Organization

Tests live under `tests/` and mirror `src/` one-to-one:

```
tests/
├── benchmark/
│   ├── optimizer-benchmark.test.ts     # BENCH-01, BENCH-02 (gated)
│   └── profile-deep.test.ts            # PERF_TRACE timing harness
├── hashing/
│   ├── naming.test.ts                  # buildDisplayName / buildSymbolName / escapeSymbol
│   └── siphash.test.ts                 # qwikHash + 209-snapshot corpus check
├── optimizer/
│   ├── snapshot-options.ts             # NOT a test file — per-snapshot option overrides
│   ├── convergence.test.ts             # The 209-snapshot convergence harness (regenerates ts-output/)
│   ├── convergence-breakdown.test.ts   # Categorizes failures (off-by-1, mostly-matching, ...)
│   ├── snapshot-batch.test.ts          # Curated subset that should already pass
│   ├── failure-families.test.ts
│   ├── extract.test.ts                 # ↔ src/optimizer/extract.ts
│   ├── transform.test.ts               # ↔ src/optimizer/transform/index.ts
│   ├── capture-analysis.test.ts        # ↔ src/optimizer/capture-analysis.ts
│   ├── jsx-transform.test.ts
│   └── ... (38 unit-test files)
└── testing/
    ├── ast-compare.test.ts             # ↔ src/testing/ast-compare.ts
    ├── batch-runner.test.ts
    ├── metadata-compare.test.ts
    └── snapshot-parser.test.ts
```

**Naming:**
- Test files: `<source-stem>.test.ts` next to where the source-stem lives (e.g. `tests/optimizer/extract.test.ts` for `src/optimizer/extract.ts`).
- A single non-test helper exists at `tests/optimizer/snapshot-options.ts` — it is excluded from `tests/**/*.test.ts` and imported by the convergence harness.

**Structure inside a file** — observed in the majority of unit test files:
```ts
import { describe, it, expect } from 'vitest';
import { extractSegments } from '../../src/optimizer/extract.js';

describe('extractSegments', () => {
  it('extracts single component$ with correct symbolName and displayName', () => {
    const source = `...`;
    const results = extractSegments(source, 'test.tsx');
    expect(results).toHaveLength(1);
    // ...
  });
});
```

Patterns:
- One top-level `describe` per exported function or feature; nested `describe`s are used in larger files (e.g. `tests/optimizer/jsx-transform.test.ts` — 608 lines, multiple groups).
- Test names are full sentences describing observable behavior, e.g. `'returns const for boolean literals'`, `'matches all hashes across the 209 snapshot corpus'`.
- No global `beforeAll` / `beforeEach` — tests construct their inputs inline. Helper functions (e.g. `findDollarArg` in `capture-analysis.test.ts`, `parseExpr` / `parseJsx` in `jsx-transform.test.ts`) are file-local.

## Snapshot Fixtures — `match-these-snaps/`

`match-these-snaps/` contains **209 golden snapshots** produced by the **Rust SWC optimizer** in [insta](https://insta.rs/) format. These are the upstream truth source.

Each `.snap` file follows the layout:
```
---
source: packages/optimizer/core/src/test.rs
assertion_line: <N>
expression: output
---
==INPUT==

<original Qwik source>

============================= test.tsx_<symbol>_<hash>.tsx (ENTRY POINT)==
<segment-module body>
Some("<JSON-escaped source map>")
/*
{ "origin": "...", "name": "...", "hash": "...", ... }    <-- segment metadata
*/

============================= test.tsx ==
<rewritten parent module>

== DIAGNOSTICS ==
[ ... JSON diagnostics array ... ]
```

Parsing: `src/testing/snapshot-parser.ts` exports `parseSnapshot(content) -> ParsedSnapshot`:
- `frontmatter` — `{ source, assertionLine, expression }`
- `input` — the `==INPUT==` block (or `null` if absent)
- `segments` — `SegmentBlock[]` with `{ filename, isEntryPoint, code, sourceMap, metadata }`
- `parentModules` — `ParentModule[]` with `{ filename, code, sourceMap }`
- `diagnostics` — parsed JSON diagnostics

**Per-snapshot options:** `tests/optimizer/snapshot-options.ts` maps snapshot test names to `TransformModulesOptions` overrides. Most snapshots use the Rust defaults (`mode: 'lib'`, `entryStrategy: 'segment'`, `minify: 'simplify'`, `transpileTs: false`, `transpileJsx: false`, `filename: 'test.tsx'`, `srcDir: '/user/qwik/src/'`). The `DEFAULT_OPTIONS` constant captures these.

**`ts-output/`** — the convergence harness writes our optimizer's output for every snapshot here on each run, so it's always in sync with `match-these-snaps/`. **Do not hand-edit.** Diff against `match-these-snaps/` to see how close TS output is to the Rust truth.

## How Equivalence Is Checked

The optimizer cannot produce byte-identical output to the Rust SWC optimizer — formatting will always differ. So tests compare **semantically** at two levels.

### 1. AST Equivalence — `compareAst`

`src/testing/ast-compare.ts` (3206 lines) exports `compareAst(expected, actual, filename)`:

```ts
import { parseSync } from 'oxc-parser';
import equal from 'fast-deep-equal';

export function compareAst(expected, actual, filename): AstCompareResult {
  let expectedResult = parseSync(filename, expected);
  let actualResult = parseSync(filename, actual);
  // ... fallback retry as .tsx if .js parse failed (some snapshots mis-flag JSX)

  // Strip position data, normalize, deep-equal
  const cleanExpected = stripPositions(expectedResult.program);
  const cleanActual = stripPositions(actualResult.program);
  normalizeProgram(cleanExpected);
  normalizeProgram(cleanActual);
  const finalExpected = stripPositions(cleanExpected);
  const finalActual = stripPositions(cleanActual);
  return { match: equal(finalExpected, finalActual), expectedParseError, actualParseError };
}
```

The strategy:
1. Re-parse both strings with `oxc-parser`.
2. Strip positional fields (`start`, `end`, `loc`, `range`).
3. Apply a long pipeline of **strictly cosmetic** normalizations to both ASTs (`normalizeProgram`):
   - Import ordering, splitting multi-specifier imports, deduping.
   - Sort import specifiers within an import.
   - Arrow body form (`x => { return y }` ≡ `x => y`).
   - Reorder independent top-level statements / expression statements.
   - QRL declaration ordering.
   - Literal forms (`void 0` ≡ `undefined`, `!0` ≡ `true`).
   - Strip `"use strict"` (implicit in ESM).
   - Unwrap single-statement blocks.
   - TS-type annotation stripping.
   - Numeric `_hf` suffix renumbering.
   - `_auto_` re-export normalization.
   - QRL var-name canonicalization (`q_qrl_4294901760` ↔ `q_<sym>_<hash>`).
   - Import alias normalization (drop unnecessary `import { X as X1 }`).
   - Dev-mode `lo`/`hi` position normalization.
   - TS enum IIFE normalization.
   - Object property reorder.
   - Inline `const X = fn; q.s(X)` → `q.s(fn)` (segment body inlining).
   - Strip dead expression statements / unused declarations / orphaned side-effect calls.
   - Multiple normalization passes (strip → re-arrow → re-strip imports).
4. Re-strip positions and run `fast-deep-equal` on the two normalized trees.

Each normalization is named after a class of cosmetic difference — for example `normalizeArrowBodies`, `canonicalizeQrlVarNames`, `inlineSegmentBodyIntoSCall`, `stripUnusedModuleLevelDeclarations`. New normalizations should follow the same shape: a free function that mutates the program in place, with a comment explaining the cosmetic difference it eliminates. **Never** add a normalization that hides a behavioral difference.

The `tests/testing/ast-compare.test.ts` suite is the truth source for what `compareAst` must accept (whitespace, alternate literal forms, JSX self-closing, equivalent arrow bodies) and what it must reject (different identifiers, parse errors).

### 2. Metadata Equivalence — `compareMetadata`

`src/testing/metadata-compare.ts` exports `compareMetadata(expected, actual)` for `SegmentMetadata` field-by-field comparison. Simple fields (`origin`, `name`, `entry`, `displayName`, `hash`, `canonicalFilename`, `path`, `extension`, `parent`, `ctxKind`, `ctxName`, `captures`) require exact equality. `loc` compares element-wise. `paramNames` / `captureNames` compare via `JSON.stringify` (preserves order — order is significant for `paramNames`).

The convergence harness uses an inlined subset of these checks (`origin`, `name`, `displayName`, `hash`, `canonicalFilename`, `ctxKind`, `ctxName`, `captures`). `tests/testing/metadata-compare.test.ts` exercises the full helper.

### 3. Hash Verification — `qwikHash`

`tests/hashing/siphash.test.ts` recomputes `qwikHash(undefined, origin, contextPortion)` for every segment in the 209-snapshot corpus and asserts the result matches the metadata `hash`. Several known edge-case files (`example_build_server`, `example_capture_imports`, `example_prod_node`, `example_qwik_react`, `example_strip_server_code`, `relative_paths`, `should_preserve_non_ident_explicit_captures`) are explicitly skipped — their hashes use a different input shape that will be addressed in a later phase.

`tests/hashing/naming.test.ts` covers `escapeSymbol`, `buildDisplayName`, and `buildSymbolName` against known-good fixed inputs.

## Convergence Harness — `tests/optimizer/convergence.test.ts`

This is the project's primary measurement tool. **It is a measurement, not a gate** — not all 209 snapshots pass yet, and the harness is structured to make progress trackable rather than to block CI.

Flow per snapshot:
1. Read `.snap` file from `match-these-snaps/`.
2. `parseSnapshot(content)`.
3. Skip if no `==INPUT==` section (some snapshots test pre-transformed code).
4. Look up per-snapshot options in `tests/optimizer/snapshot-options.ts` via `getSnapshotTransformOptions(testName, parsed.input)`.
5. Call `transformModule(options)`.
6. Write the formatted result to `ts-output/<snap>.snap` (always — keeps `ts-output/` in sync).
7. `compareAst` parent module + each segment.
8. Compare core metadata fields per segment.
9. Classify result: `fullPass` / `parentOnlyFail` / `segmentOnlyFail` / `fullFail` / `noInput` / `error`.
10. `expect(parentMatches).toBe(true)` and `expect(segmentsMatch).toBe(true)` so individual failures appear in the test report.

A trailing `'convergence summary'` test prints aggregate counters (total / fullPass / parentOnlyFail / segmentOnlyFail / fullFail / noInput / error) and pass-rate %.

**Curated subsets** in `tests/optimizer/snapshot-batch.test.ts` track which snapshots are expected to fully pass right now (`fullMatchSnapshots`, `parentMatchSnapshots`, `jsxParentMatchSnapshots`). Adding a snapshot here is the way to mark "this used to fail and now passes — protect against regressions".

**Failure categorization** in `tests/optimizer/convergence-breakdown.test.ts` slots each failing snapshot into `'off-by-1' | 'mostly-matching' | 'major-diff' | 'parse-error'`, useful when planning what to fix next.

## Benchmark Suites — `tests/benchmark/`

Two benchmarks compare against the **native SWC NAPI binding**:
- **BENCH-01:** Full Qwik monorepo. Limit: TS ≤ 1.15× SWC wall-time.
- **BENCH-02:** Worst-case single file (`packages/qwik/src/core/tests/component.spec.tsx`). Limit: TS ≤ 1.5× SWC.

Both use a warmup run (`WARMUP_RUNS = 1`) followed by `MEASURED_RUNS = 2`, taking the **minimum** elapsed time per side, then asserting `tsTime / swcTime <= LIMIT`.

**Gating** (recently added — see commit `45e6a55`): the benchmark file reads `process.env.QWIK_SWC_BINDING`. If unset/empty:
```ts
const QWIK_SWC_BINDING = process.env.QWIK_SWC_BINDING;
if (!QWIK_SWC_BINDING || QWIK_SWC_BINDING === '') {
    describe.skip('BENCH-00: QWIK_SWC_BINDING was not set, skipping benchmark tests.', () => {})
} else {
    const swcBinding = require(QWIK_SWC_BINDING);
    // ... BENCH-01, BENCH-02 ...
}
```
This prevents test failures when no SWC binding is available locally — a placeholder `BENCH-00` describe is reported as skipped instead.

When the binding IS set, BENCH-01 and BENCH-02 are **further** gated on `process.env.BENCH === '1'`:
```ts
const runBenchmarks = process.env['BENCH'] === '1';
const describeFn = runBenchmarks ? describe : describe.skip;
```
So they run only with both env vars set:
```bash
BENCH=1 QWIK_SWC_BINDING=/path/to/binding.node \
  pnpm vitest run tests/benchmark/optimizer-benchmark.test.ts --no-file-parallelism
```

`tests/benchmark/profile-deep.test.ts` is a much smaller harness that runs `transformModule` once with `process.env['PERF_TRACE'] = '1'` set, used for ad-hoc deep-timing runs (no assertion on time budget).

## Test Coverage

**No coverage tool is configured.** Vitest's `--coverage` works out of the box (V8 by default), but no script wires it up and no thresholds are enforced.

The 209-snapshot corpus is the project's true coverage indicator — pass-rate against `match-these-snaps/` measures real-world feature completeness better than line coverage.

## CI Hooks

**No CI configuration committed.** No `.github/`, `.circleci/`, `.gitlab-ci.yml`, or similar. Pre-commit hooks: none configured (no `husky/`, `lefthook.yml`, etc.). Tests are expected to be run manually with `pnpm test`.

The benchmark file's docstring describes BENCH-01/02 as "CI-enforceable regression gates" — language indicating future intent, but none is currently wired up.

## Common Patterns You Should Follow

**Adding a unit test for a new helper:**
```ts
// tests/optimizer/<feature>.test.ts
import { describe, it, expect } from 'vitest';
import { myFunction } from '../../src/optimizer/<feature>.js';

describe('myFunction', () => {
  it('does X for input Y', () => {
    expect(myFunction('Y')).toBe('X');
  });
});
```
Co-locate the file at `tests/optimizer/<feature>.test.ts` mirroring `src/optimizer/<feature>.ts`. Use multi-line backtick template literals for source-code fixtures (see every test file under `tests/optimizer/`).

**Adding an integration test using a snapshot input:**
```ts
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseSnapshot } from '../../src/testing/snapshot-parser.js';
import { compareAst } from '../../src/testing/ast-compare.js';
import { transformModule } from '../../src/optimizer/transform/index.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SNAP = join(__dirname, '../../match-these-snaps/qwik_core__test__example_1.snap');
const parsed = parseSnapshot(readFileSync(SNAP, 'utf-8'));
const result = transformModule({
  input: [{ path: 'test.tsx', code: parsed.input! }],
  srcDir: '/user/qwik/src/',
  mode: 'lib',
});
expect(compareAst(parsed.parentModules[0].code, result.modules[0].code, 'test.tsx').match).toBe(true);
```

**Async testing:** No async/await is currently used in the unit-test suite. Benchmarks use `async`/`await` for the SWC binding's promise-returning `transform_modules`. Use `async () => { ... }` only when an API genuinely returns a Promise.

**Error testing:** Construct the input that should error and assert on the resulting `Diagnostic` rather than `expect(() => fn()).toThrow(...)`. The optimizer rarely throws — it emits diagnostics. Example:
```ts
const result = transformModule({ input: [{ path: 'test.tsx', code }], srcDir: '.' });
const c02 = result.diagnostics.find(d => d.code === 'C02');
expect(c02).toBeDefined();
expect(c02!.message).toContain("'hola'");
```
Direct constructor tests for diagnostics use the emit functions (`emitC02`, `emitC05`, `emitPassiveConflictWarning`) — see `tests/optimizer/diagnostics.test.ts`.

**Skip patterns:** Test files use:
- `describe.skip(...)` for env-gated suites (benchmarks).
- A `KNOWN_EDGE_CASE_FILES` set + `continue` inside a loop for the snapshot-driven tests, when a small list of fixtures is known to use a different code path.

Avoid `it.skip` / `it.only` — they are not used in the codebase and should not slip into committed code.

---

*Testing analysis: 2026-05-05*

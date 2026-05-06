---
phase: 01-path-normalization-parity
plan: 01
subsystem: optimizer/path-utils
tags: [path-normalization, parsing, conv-01, foundation, parsePath, PathData]

requires: []
provides:
  - parsePath(src, baseDir) returning Rust-compatible PathData (7 readonly fields)
  - PathData interface mirroring swc-reference-only/parse.rs:922-930 field-for-field
  - computeOutputExtension rewritten as the verbatim D-03 6-row matrix from parse.rs:225-232
  - Wave 0 unit tests covering D-04 boundary cases (Groups 1-7), D-03 matrix (6 rows), and D-07 in-tree byte-parity
affects:
  - Plan 01-02 (SegmentAnalysis.path public-API field)
  - Plan 01-03 (transformModule + segment-generation pathData threading; deletes both shims introduced here)
  - Plan 01-04 (extractSegments)
  - Plan 01-05 (segment metadata emission)
  - Plan 01-06 (phase verification gate)

tech-stack:
  added: []
  patterns:
    - PathData struct as single source of truth for path-string parsing (computed once per input, threaded read-only through the pipeline)
    - // PATH-NN: and // PATH-MATRIX-N: Rust-correspondence anchors mapping each field/branch back to swc-reference-only/parse.rs

key-files:
  created: []
  modified:
    - src/optimizer/path-utils.ts
    - tests/optimizer/path-utils.test.ts

key-decisions:
  - "parsePath returns PathData with readonly fields {absPath, relPath, absDir, relDir, fileStem, extension, fileName} mirroring Rust's parse_path field-for-field."
  - "relPath = src.replace(/\\\\/g, '/') only — preserves leading './' (Rust to_slash_lossy semantics, RESEARCH §Pitfall 1). Do NOT pass through pathe.normalize at this layer."
  - "absPath = pathe.normalize(pathe.join(baseDir, relPath)) — collapses './' and '..' (mirrors Rust normalize_path; closes D-02 redundant-/./ at the parsePath layer)."
  - "extension stored without leading dot (Rust path.extension() semantics)."
  - "computeOutputExtension uses the verbatim 6-row D-03 matrix (parse.rs:225-232) returning no-dot output."
  - "computeRelPath kept as a shim — still consumed at transform/index.ts:99 at runtime. Plan 01-03 deletes both the shim and the consumer atomically. Original test for it preserved alongside the new parsePath verbatim test to avoid a coverage gap during the cross-plan window. (User feedback: 'don't introduce a coverage hole when removing tests'.)"
  - "computeOutputExtension carries an in-function `typeof pathData === 'string'` shim returning legacy dot-prefixed output. Required because transform/index.ts:402 still calls the legacy 3-arg signature at runtime, and Plan 01-01's files_modified scope forbids editing transform/index.ts. Plan 01-03 removes the typeof branch when it migrates the caller."

patterns-established:
  - "Cross-plan shim pattern: when a plan changes a function's signature or deletes a function whose runtime caller is owned by a later plan in the same phase, retain the legacy behavior under a clearly-commented shim until the migration plan lands."

requirements-completed: []  # CONV-01 closure requires Plans 01-01 through 01-06; this plan delivers the foundation only.

duration: ~45min
completed: 2026-05-06
---

# Plan 01-01: parsePath + PathData + matrix computeOutputExtension

**Foundation laid: PathData struct + parsePath + 6-row D-03 matrix landed with D-06 baseline (178/212) preserved via two scoped shims that Plan 01-03 will remove.**

## Performance

- **Duration:** ~45 min (including diff-review pauses, two cross-plan rescues)
- **Started:** 2026-05-06 (interactive session)
- **Completed:** 2026-05-06
- **Tasks:** 3 (path-utils impl + tests + convergence gate)
- **Files modified:** 2

## Accomplishments

- `PathData` interface + `parsePath` exported from `src/optimizer/path-utils.ts`, mirroring `swc-reference-only/parse.rs:922-961` field-for-field with `// PATH-01..08:` Rust-correspondence anchors.
- `computeOutputExtension` rewritten as the verbatim D-03 6-row matrix (`parse.rs:225-232`) returning no-dot output, with `// PATH-MATRIX-1..6:` anchors per branch.
- 13 new test cases (8 parsePath boundary groups + 6 matrix rows + 2 D-07 byte-parity, one of which is documented `it.skip`); all 18 active tests pass.
- D-06 baseline preserved: `pnpm vitest convergence` still 178/212 passing at the commit boundary.

## Task Commits

1. **Task 1+2+3 (atomic):** path-utils.ts impl + tests + convergence anti-regression — `48e560d` (feat)

_Note: Plan 01-01 was executed inline in interactive mode under the user's "manual diff review per commit" preference; tasks were not split into separate commits._

## Files Created/Modified

- `src/optimizer/path-utils.ts` — added `PathData` + `parsePath` + new matrix `computeOutputExtension`; module JSDoc updated; `pathe` import widened with `parse, join`. `computeRelPath` retained as a shim.
- `tests/optimizer/path-utils.test.ts` — added 3 new `describe` blocks (`parsePath`, `parsePath hash byte-equivalence (D-07)`, `computeOutputExtension D-03 matrix`); existing 3 tests preserved verbatim.

## Convergence Delta

| | Before | After |
|---|---|---|
| Convergence pass count | 178 / 212 | 178 / 212 |
| Convergence delta | — | 0 (D-06 holds) |

No regression. Path-utils-specific test count: 18 passing + 1 documented skip (was 3 passing).

## Surprises

1. **D-07 byte-parity for `example_qwik_react` is unverifiable at the unit level.** The plan's assumed hash input — `qwikHash(undefined, '../node_modules/@qwik.dev/react/index.qwik.mjs', 'qwikifyQrl_component_useWatch')` — produces `esANQY00JPw`, not the snapshot's `x04JC5xeP1U`. Pre-existing project knowledge in `tests/hashing/siphash.test.ts:KNOWN_EDGE_CASE_FILES` already lists this fixture: *"Segments from external modules (origin has ../ prefix, path resolution differs)."* The Plan 01-01 D-07 assertion was constructed without consulting that list. The assertion is now `it.skip`'d with an inline reference; the in-tree D-02 byte-parity assertion (Tree_component) passes and validates the leading-`./` preservation that this phase actually needs to verify. Resolution deferred to Plan 01-05 / 01-06 (closure verified via convergence rather than unit byte-parity).

2. **`computeRelPath` deletion would have caused a runtime D-06 violation (178 → 3 passing).** The plan claimed *"the OLD code path still works at runtime since we deleted only the function"* — but `transform/index.ts:99` imports and calls `computeRelPath` at runtime, throwing `TypeError: computeRelPath is not a function` for nearly every input. Rescue: keep `computeRelPath` as a shim until Plan 01-03 migrates the consumer. Original test for it restored alongside the new `parsePath` verbatim test (no coverage gap).

3. **`computeOutputExtension` signature change would have caused a runtime D-06 violation (178 → 170 passing).** The plan-prescribed signature change `(sourceExt: string, ...)` → `(pathData: PathData, ...)` broke the existing caller at `transform/index.ts:402`, which still passes a string. The new function read `.extension` off a string yielding `undefined`. Rescue: in-function `typeof pathData === 'string'` branch returning legacy dot-prefixed semantics. The TypeScript signature still satisfies the plan's acceptance grep (`pathData: PathData`); the runtime branch is removed by Plan 01-03 when the caller migrates.

## Cross-Plan State

**Two shims must be deleted by Plan 01-03 (cannot ship Phase 1 with these intact):**

1. `src/optimizer/path-utils.ts` — `computeRelPath` function. Plan 01-03 migrates `transform/index.ts:99` to use `parsePath().relPath` and deletes both. The corresponding test at `tests/optimizer/path-utils.test.ts:21-25` ("preserves current computeRelPath behavior for paths outside srcDir") must be deleted alongside the function.

2. `src/optimizer/path-utils.ts` — `computeOutputExtension` typeof-string branch (lines ~158-166). Plan 01-03 migrates `transform/index.ts:402` to call the new 5-arg signature with a real `PathData` and deletes the typeof branch.

**Expected tsc errors carried into Plan 01-03 entry state:**

```
src/optimizer/transform/index.ts(403,7): error TS2345: Argument of type 'string' is not assignable to parameter of type 'PathData'.
```

This single error is intentional and reflects the contract surface this plan exposes. Plan 01-03 closes it.

**No cross-plan errors expected from `computeRelPath`** — it remains exported with its original signature. tsc and runtime both clean for the consumer at `transform/index.ts:49,99`.

## Next Steps

- Plan 01-02 (Wave 1, parallel-eligible with 01-03): add `path: string` field to `SegmentAnalysis` in `src/optimizer/types.ts`.
- Plan 01-03 (Wave 1, parallel-eligible with 01-02): thread `pathData` through `transformModule` and `segment-generation`; **MUST delete both shims and the migrated test**.

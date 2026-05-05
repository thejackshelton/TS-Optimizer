# Phase 1: Path normalization parity - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-05
**Phase:** 1-path-normalization-parity
**Areas discussed:** Abstraction shape, Spillover policy, Extension-propagation rule, Testing approach

---

## Abstraction shape

How should we shape the path layer? Rust computes a `PathData { abs_path, rel_path, abs_dir, rel_dir, file_stem, extension, file_name }` once at entry and threads it through. The TS version currently has scattered helpers (`computeRelPath`, `getBasename`, `getDirectory`, `computeOutputExtension`) called ad-hoc.

| Option | Description | Selected |
|--------|-------------|----------|
| Mirror Rust PathData (Recommended) | Add a `parsePath(src, baseDir)` in path-utils.ts that returns a typed `PathData` object matching Rust field-for-field. Compute once in `transformModule`, thread through to extract.ts and segment-generation.ts. Replaces ad-hoc calls. Easier to port future Rust path changes; gives downstream phases (5, 6) a stable handle. Slightly larger diff. | ✓ |
| Fix scattered helpers in place | Keep the existing helper signatures, fix the bugs inside each. Smaller diff, no API churn, but the Rust↔TS mapping stays implicit. | |

**User's choice:** Mirror Rust PathData
**Notes:** Aligns with the project-wide preference for explicit Rust↔TS mapping (see PROJECT.md "Reference source of truth" + STATE.md "Hashing"). Single integration site at `transformModule` keeps the change localized.

---

## Spillover policy

Cluster 7 has a related path bug: `./node_modules/qwik-tree/index.qwik.jsx` becomes `/user/qwik/src/./node_modules/...` with a redundant `./` (test `root_level_self_referential_qrl_inline`). Same root code, different symptom. Fix now or defer to Phase 7?

| Option | Description | Selected |
|--------|-------------|----------|
| Fix opportunistically in Phase 1 (Recommended) | If our `parsePath` faithfully mirrors Rust's `normalize_path` (which collapses redundant `./` and `..`), this fix falls out for free. Triage explicitly notes Cluster 7 entries close as side-effects of upstream fixes. Document the closure in the phase plan. | ✓ |
| Strict: only the 4 CONV-01 tests | Hold the line on success criterion #4 ("localized to the listed files") and let Phase 7 deal with anything not in CONV-01's 4 tests. Cleaner phase boundary but creates a known-but-deferred bug touching the same code. | |

**User's choice:** Fix opportunistically in Phase 1
**Notes:** The fix is structurally part of `normalize_path` parity, not a separate intervention. Phase 7 will re-baseline the residual count and is allowed to find this already gone.

---

## Extension-propagation rule

Extension propagation: Rust's `parse_path` always uses the input file's extension. TS currently has `computeOutputExtension(sourceExt, transpileTs, transpileJsx)` that overrides to `.js`/`.ts` based on transpile flags. SWC parity requires echoing input extension. What about the transpile-flag overrides?

| Option | Description | Selected |
|--------|-------------|----------|
| Verify Rust behavior, then mirror exactly (Recommended) | Researcher reads `swc-reference-only/transform.rs` to confirm what SWC does when `transpile_ts`/`transpile_jsx` are set with input `.mjs`/`.tsx`. Match whatever Rust does — including the override logic if Rust has equivalent. Don't guess; the snapshots in `match-these-snaps/` are the oracle. | ✓ |
| Drop overrides entirely — always echo input extension | Strip `computeOutputExtension`'s transpile-flag branches; segments emit with the input's extension verbatim. | |
| Keep overrides, only fix `.mjs`/`.qwik.mjs` propagation | Minimal: fix the path so `.mjs` flows through when no transpile flags, leave the transpile-true branches untouched. | |

**User's choice:** Verify Rust behavior, then mirror exactly
**Notes:** Hard parity decision deferred to research phase. Plan must encode the verified rule as an explicit mapping table — no implicit logic.

---

## Testing approach

Beyond `pnpm vitest convergence`, do we add path-utils unit tests?

| Option | Description | Selected |
|--------|-------------|----------|
| Add unit tests for parsePath boundary cases (Recommended) | New `tests/optimizer/path-utils.test.ts` covering the boundary cases. Locks parity behavior so future refactors can't regress it silently. Convergence remains the north star. | ✓ |
| Convergence-only | Iterate against `pnpm vitest convergence` exclusively. Lighter; ROADMAP says convergence is the only gate. | |

**User's choice:** Add unit tests for parsePath boundary cases
**Notes:** Belt-and-suspenders — the function is small, pure, and lives at the foundation of the pipeline; cheap insurance against silent regressions in future phases.

---

## Final readiness check

| Option | Description | Selected |
|--------|-------------|----------|
| Ready for context | Decisions are clear. Write CONTEXT.md and proceed to /gsd-plan-phase 1. | ✓ |
| Explore more gray areas | Surface more decisions — e.g., handling of `srcDir` defaults, `abs_path` base resolution, hash-input bytes, or backward-compat with the 178 currently-passing tests. | |

**User's choice:** Ready for context
**Notes:** The remaining considerations (`srcDir` defaults, hash-input byte parity, no-regression discipline) were folded as locked operating constraints (D-05, D-06, D-07) rather than spawning new discussion threads.

---

## Claude's Discretion

- Internal `PathData` typing style — `interface` per CONVENTIONS.md.
- Migration strategy for the scattered helpers — atomic vs gradual; pick whichever reads cleanly.
- Naming — `parsePath` recommended; alternatives like `buildPathData` or `resolvePathData` acceptable if better justified in the plan.

## Deferred Ideas

- Cluster 7 path-adjacent symptoms beyond D-02 — defer to Phase 7.
- Aggressive refactoring of path callers in unrelated files — capture as TD note in `.planning/codebase/CONCERNS.md`, do not do here.
- Source maps and path-operation perf tuning — already out-of-scope per PROJECT.md.

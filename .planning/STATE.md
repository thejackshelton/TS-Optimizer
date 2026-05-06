---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
last_updated: "2026-05-06T19:00:00.000Z"
progress:
  total_phases: 7
  completed_phases: 0
  total_plans: 6
  completed_plans: 1
  percent: 2.4
---

# State: Qwik Optimizer (TypeScript)

**Last Updated:** 2026-05-06
**Status:** Executing Phase 1 (Plan 01-01 complete; Wave 1 next)

## Project Reference

**Project:** Qwik Optimizer (TypeScript) — drop-in TS replacement for Qwik's Rust/SWC NAPI optimizer.
**Core Value:** AST-semantic equivalence with the SWC reference optimizer; QRL hashes match Rust `DefaultHasher` (SipHash-1-3 with zero keys) byte-for-byte; convergence tests prove this by deep-equal'ing normalized ASTs from both optimizers.
**Current Focus:** Drive `pnpm vitest convergence` from 178/212 → 212/212 with zero regressions in the 178 currently-passing tests, by closing the seven CONV-* clusters identified in `.planning/research/CONVERGENCE-TRIAGE.md`.

## Current Position

**Milestone:** Convergence to 212/212
**Phase:** Phase 1 — Path normalization parity (executing)
**Plan:** 01-02 (Wave 1, next) — SegmentAnalysis.path public-API field
**Resume file:** `.planning/phases/01-path-normalization-parity/01-02-PLAN.md`
**Status:** executing
**Mode:** interactive (manual diff review before each commit)

**Progress:**

```
Phase 1: [ ] not started
Phase 2: [ ] not started
Phase 3: [ ] not started
Phase 4: [ ] not started
Phase 5: [ ] not started
Phase 6: [ ] not started
Phase 7: [ ] not started

0/7 phases complete
```

**Convergence tracker:**

- Baseline (2026-05-05): 178/212 passing, 34 failing.
- Target: 212/212 passing.
- Current: 178/212 (Plan 01-01 landed at `48e560d` — foundation only; D-06 baseline preserved via two cross-plan shims that Plan 01-03 will remove).

## Performance Metrics

| Metric | Baseline | Current | Target |
|--------|----------|---------|--------|
| Convergence pass rate | 178/212 | 178/212 | 212/212 |
| 178-passing regression count | 0 | 0 | 0 (must stay 0 every phase) |
| Convergence delta per phase | n/a | n/a | tracked in each phase's transition |
| QRL hash parity | byte-equal to Rust DefaultHasher | byte-equal | byte-equal (must not break) |
| API shape vs NAPI | drop-in | drop-in | drop-in (must not break) |

Benchmarks (`QWIK_SWC_BINDING` gate, monorepo within 1.15× SWC, worst-case file within 1.5× SWC) remain green-of-record but are not a phase gate this milestone.

## Accumulated Context

### Key Decisions

- **Phase shape:** 7 phases, one per CONV-* cluster. User chose this granularity explicitly during `/gsd-new-project`. Do not batch clusters into fewer phases. Do not split a cluster across phases.
- **Phase order:** Cheap & isolated first (paths → stripped placeholders → identifier collisions), structural last (`_rawProps` → const-propagation), residual sweep at the end. Matches the "Suggested phase order" section of `.planning/research/CONVERGENCE-TRIAGE.md`.
- **Operating mode:** `interactive` — user reviews every diff before commit. No auto-commits.
- **Reference source of truth:** `./swc-reference-only/` (Rust). Read-only. The Rust source is the answer to any semantic question about SWC behaviour.
- **Hashing:** Always go through `qwikHash()` in `src/hashing/siphash.ts`. SipHash-1-3 with `(0,0,0,0)` keys. Do not substitute SHA-256, MurmurHash, or a custom impl.
- **Parser:** oxc-parser with `experimentalRawTransfer: true` (via `RAW_TRANSFER_PARSER_OPTIONS` in `src/ast-types.ts:100`). Single parse per input where possible (DESIGN.md "parse once" directive).
- **Codegen:** magic-string surgical edits over original text. Never reprint from AST. `oxc-transform` only as a final TS-stripping pass over already-rewritten text.

### Active Regression to Close in Phase 4

- **A1.** `<Component on*$={fn}>` JSX attribute mis-classified as `'eventHandler'` instead of `'jSXProp'` at `src/optimizer/extract.ts:583-590`. The marker-call branch at `:489-495` is already correct. Fix: drop the on-prefix gate. ~3-line change. Documented in `.planning/codebase/CONCERNS.md > A1` and pre-investigated in `.planning/debug/yuku-parser-0-5-0-regression*.md`.

### Known Pending Items (not requirements; carried forward as awareness)

- Tech debt cataloged in `.planning/codebase/CONCERNS.md` (TD-1 through TD-12, FR-1 through FR-5, PERF-1, PERF-2, HS-1 through HS-3, SEC-1, SEC-2, AR-3, AP-1 through AP-3). Most are out-of-scope for this milestone but may surface as side-effects of phase work — flag in plans, do not silently fold in.
- TD-11 (`classifyConstness` for member expressions) is in scope and is part of Phase 6's success criteria.

### Open Todos

None. Plans for individual phases will be created via `/gsd-plan-phase 1` etc.

### Blockers

None.

## Session Continuity

**What was last accomplished:**

- 2026-05-05: `/gsd-new-project` initialization complete. PROJECT.md, REQUIREMENTS.md, codebase maps, research triage, config.json all in place. Roadmap: 7 phases, one per CONV-* cluster.
- 2026-05-05: `/gsd-discuss-phase 1` — CONTEXT D-01..D-07 captured; DISCUSSION-LOG preserved alternatives.
- 2026-05-05: `/gsd-plan-phase 1` — decomposed into 6 plans across 5 waves.
- 2026-05-06: Plan 01-01 landed at `48e560d`. parsePath + PathData + matrix computeOutputExtension. D-06 (178/212) preserved. Two cross-plan shims (computeRelPath; computeOutputExtension typeof branch) carried forward — Plan 01-03 must delete both.

**What's next:**

- Plan 01-02 (Wave 1) — add `path: string` to `SegmentAnalysis` in `src/optimizer/types.ts`. Parallel-eligible with 01-03.
- Plan 01-03 (Wave 1) — thread `pathData` through `transformModule` + `segment-generation`; **MUST delete the two shims and the migrated test** introduced by Plan 01-01.
- Plans 01-04 → 01-06 — extract.ts migration, segment metadata emission, phase verification gate.

**Useful commands:**

- `pnpm vitest convergence` — north-star metric (must end at 212/212).
- `pnpm test` — full test suite (`ts-output/*.snap` is auto-regenerated; revert with `git checkout -- ts-output/` if needed — see CONCERNS TD-1).
- `pnpm test:watch` — watch mode.

**Useful files when planning each phase:**

- `.planning/research/CONVERGENCE-TRIAGE.md` — root-cause and file pointers per cluster.
- `.planning/codebase/ARCHITECTURE.md` — pipeline structure and where each phase lands.
- `.planning/codebase/CONCERNS.md` — risk areas and known fragility for each phase's surface.
- `./swc-reference-only/` — Rust reference (read-only).

---
*State initialized: 2026-05-05 by gsd-roadmapper*

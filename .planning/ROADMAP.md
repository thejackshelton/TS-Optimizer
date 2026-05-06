# Roadmap: Qwik Optimizer (TypeScript)

**Created:** 2026-05-05
**Granularity:** standard (7 phases — one per CONV-* cluster, per user direction)
**Coverage:** 7/7 v1 requirements mapped
**North-star metric:** `pnpm vitest convergence` — drive 178/212 → 212/212 with no regression in the 178 currently-passing tests.

## Milestone Goal

Close the seven structural parity gaps catalogued in
`.planning/research/CONVERGENCE-TRIAGE.md` so the TS optimizer's emitted
output is AST-semantically equivalent to Qwik's Rust/SWC reference for
every snapshot in `match-these-snaps/`. QRL hashes, segment counts, segment
metadata (`parent`, `ctxName`, `path`, `extension`, `loc`), and parent
module shape all converge on the SWC reference.

## Phase Order Rationale

Phase order follows the recommendation in `CONVERGENCE-TRIAGE.md > Suggested
phase order`: cheap and isolated work first, structural work last, residual
sweep at the end. Earlier phases destabilise diffs less for downstream
phases (closing path normalisation first means Cluster 7's path-shaped
sub-failures surface as themselves and not as cascaded path errors).

No deviation from the triage's recommended order was needed — the
dependencies the triage flags (Cluster 4 unblocks Cluster 1's segment-side
work, Cluster 5 unblocks Cluster 7 path-shaped residuals) are already
respected by the order below.

## Phases

- [ ] **Phase 1: Path normalization parity** — TS optimizer preserves the full relative directory portion of input filenames (no basename collapse, correct extension propagation) so 4 path-shaped convergence tests converge on SWC.
- [ ] **Phase 2: Stripped-segment placeholder emission** — TS optimizer emits `export const X = null` placeholders with `loc: [0, 0]` for `stripCtxName`/`stripEventHandlers` extractions instead of skipping them.
- [ ] **Phase 3: Identifier-collision rewriting for injected imports** — TS optimizer detects user identifiers that shadow injected QRL imports (`qrl`, `componentQrl`, etc.) and renames the user binding to a numeric suffix so injected imports resolve correctly.
- [ ] **Phase 4: Captures-vs-paramNames classification** — TS optimizer correctly classifies segments inside non-`component$` lightweight functional components and `<Cmp on*$>` JSX attributes, closing the active yuku/A1 regression.
- [ ] **Phase 5: `_rawProps` rewriting for destructured/aliased props** — TS optimizer rewrites destructured component arguments through a single `_rawProps` parameter for nested, aliased, defaulted and rest-spread destructure shapes.
- [ ] **Phase 6: Const propagation and folding across the segment boundary** — TS optimizer inlines parent-scope literal `const`s into consuming segments, folds compile-time conditionals, and drops dead `_auto_` re-exports.
- [ ] **Phase 7: Residual one-offs sweep** — TS optimizer closes the residual convergence failures that don't fit clusters 1-6 (count may shrink as 1-6 cascade through dependent failures).

## Phase Details

### Phase 1: Path normalization parity
**Goal**: TS optimizer normalizes input filenames consistently with the Rust SWC reference so `../node_modules/...` and subdirectory paths flow through to parent module headers, segment filenames, segment `path` metadata and segment `extension` metadata without basename collapse.
**Depends on**: Nothing (first phase; isolated to `src/optimizer/path-utils.ts` and the segment-metadata construction sites).
**Requirements**: CONV-01
**Success Criteria** (what must be TRUE):
  1. The 4 currently-failing path-normalization tests in `pnpm vitest convergence` (`example_qwik_react`, `example_qwik_react_inline`, `example_qwik_router_client`, `example_strip_client_code`) move from failing to passing — or, where another cluster's gap dominates, the path delta is no longer present in the failure diff.
  2. For an input filename like `../node_modules/@qwik.dev/react/index.qwik.mjs`, the parent module header preserves the full prefix and `SegmentAnalysis.path` equals `'../node_modules/@qwik.dev/react'` (not `''`), and `SegmentAnalysis.extension` equals `'mjs'` (not `'js'`).
  3. The 178 currently-passing convergence tests still pass — no regression introduced in `pnpm vitest convergence`.
  4. The change is localized to `src/optimizer/path-utils.ts`, `src/optimizer/extract.ts` (segment filename construction) and the path threading in `src/optimizer/transform/index.ts` / `src/optimizer/transform/segment-generation.ts`; other modules are unchanged. (User-confirmed widening: `src/optimizer/types.ts` is added for the `SegmentAnalysis.path` public field.)

**Plans:** 6 plans

Plans:
- [x] 01-01-PLAN.md — Foundation: `parsePath` + `PathData` interface + Wave 0 unit tests (D-04 boundary cases + D-07 hash byte-parity + D-03 matrix) — `48e560d`
- [ ] 01-02-PLAN.md — Public type addition: `SegmentAnalysis.path: string` + smoke convergence (Open Q3 disposition)
- [ ] 01-03-PLAN.md — Compute & thread `PathData` through `transformModule`; replace `computeRelPath`/`getExtension` callers; delete downgrade loop; did_transform-gated parent path; SegmentGenerationContext.pathData
- [ ] 01-04-PLAN.md — Migrate `extract.ts` to consume PathData + qrlOutputExt; delete `determineExtension`; uniform extension assignment across all push sites
- [ ] 01-05-PLAN.md — Populate emission sites: `SegmentAnalysis.path = ctx.pathData.relDir`; drop `leadingDot` strip; rebuild `segmentModule.path`; close 4 CONV-01 tests
- [ ] 01-06-PLAN.md — Final convergence gate + Phase 1 closure (PHASE-SUMMARY.md, STATE.md/ROADMAP.md updates, manual diff review checkpoint)

### Phase 2: Stripped-segment placeholder emission
**Goal**: When `stripCtxName` and/or `stripEventHandlers` apply, TS optimizer emits a placeholder segment file (`export const <symbolName> = null;`, `loc: [0, 0]`, original `ctxKind`/`ctxName` preserved) instead of dropping the extraction or emitting the un-stripped body. The `serverLoader$(handler)` named-callback display-name bug (which shares this code path) is fixed in the same change.
**Depends on**: Phase 1 (so any path-shaped delta in stripped-segment tests is already gone and the diff that remains is genuinely a stripping/naming gap).
**Requirements**: CONV-02
**Success Criteria** (what must be TRUE):
  1. The 3 currently-failing stripped-placeholder tests (`example_strip_server_code`, `example_strip_client_code`, `example_noop_dev_mode`) move from failing to passing in `pnpm vitest convergence`.
  2. For an input like `serverLoader$(handler)` under `stripCtxName: ['server']`, the emitted segment file body is exactly `export const s_<hash> = null;` with `loc: [0, 0]`, `captures: false`, and the segment is named after the `handler` identifier — not after the enclosing component context.
  3. The 178 currently-passing convergence tests still pass — no regression introduced.
  4. The change is localized to `src/optimizer/strip-ctx.ts`, `src/optimizer/transform/segment-generation.ts` (the two emission sites at `:392` and `:812`), and option handling in `src/optimizer/types.ts`; other modules unchanged.
**Plans**: TBD

### Phase 3: Identifier-collision rewriting for injected imports
**Goal**: Before output assembly, TS optimizer scans the parent module for user identifiers that collide with the names of imports it would inject (`qrl`, `componentQrl`, `inlinedQrl`, `_jsxSorted`, `_captures`). On collision, the user's binding is renamed to `<name>1` everywhere it appears, and the injected import keeps the canonical name.
**Depends on**: Phase 2 (so the stripped-emission and naming work has settled — the collision pre-pass adds another transformation layer over the parent module that benefits from a stable segment count).
**Requirements**: CONV-03
**Success Criteria** (what must be TRUE):
  1. The 1 currently-failing collision test (`example_qwik_conflict`) moves from failing to passing in `pnpm vitest convergence`.
  2. When user code imports `qrl` from `@qwik.dev/core/what` or declares `const componentQrl = ...`, the emitted output renames the user's binding to `qrl1` / `componentQrl1` (with all references rewritten) and the injected `import { qrl } from "@qwik.dev/core"` and `import { componentQrl } from "@qwik.dev/core"` lines are both present.
  3. The 178 currently-passing convergence tests still pass — no regression introduced.
  4. The change is localized to a new pre-pass in `src/optimizer/rewrite/index.ts` (or a new dedicated file) plus `src/optimizer/rewrite-imports.ts`, `src/optimizer/marker-detection.ts`, and `src/optimizer/rewrite/output-assembly.ts`; other modules unchanged.
**Plans**: TBD

### Phase 4: Captures-vs-paramNames classification (closes A1 regression)
**Goal**: TS optimizer keys the captures-vs-paramNames decision off "is the closest enclosing extraction a `component$`/`componentQrl`?" rather than "is a loop present?", so segments inside lightweight functional components emit a `_captures = useLexicalScope()` prologue instead of the loop-hoisted `(_, _1, slotParam)` pattern. The active A1 regression at `src/optimizer/extract.ts:583-590` (component `on*$` JSX attributes mis-classified as `eventHandler`) is closed in the same change.
**Depends on**: Phase 3 (so injected-import rewriting is settled before we touch the captures emission path that interacts with `useLexicalScope`/`_captures` imports).
**Requirements**: CONV-04
**Success Criteria** (what must be TRUE):
  1. The 5 currently-failing classification tests (`example_lightweight_functional`, `example_functional_component_capture_props`, `example_immutable_analysis`, `example_qwik_react`, `example_component_with_event_listeners_inside_loop`) move from failing to passing in `pnpm vitest convergence`.
  2. The active A1 regression in `src/optimizer/extract.ts:583-590` is closed: for `<Component on*$={fn}>`, `ExtractionResult.ctxKind` equals `'jSXProp'` (not `'eventHandler'`), matching the marker-call branch at `:489-495` and SWC. The corresponding regression doc files in `.planning/debug/yuku-parser-0-5-0-regression*.md` can be archived/closed.
  3. For a non-`component$` lightweight functional component with a captured outer-scope identifier, the emitted segment body starts with `import { _captures } from "@qwik.dev/core"` and a `const x = _captures[0], y = _captures[1];` prologue — not a `(_, _1, x, y)` parameter list.
  4. The 178 currently-passing convergence tests still pass — no regression in `pnpm vitest convergence`.
  5. The change is localized to `src/optimizer/extract.ts`, `src/optimizer/capture-analysis.ts`, `src/optimizer/transform/event-capture-promotion.ts`, and `src/optimizer/segment-codegen/body-transforms.ts`; other modules unchanged.
**Plans**: TBD

### Phase 5: `_rawProps` rewriting for destructured/aliased props
**Goal**: TS optimizer rewrites destructured / aliased / defaulted / rest-spread component arguments to flow through a single `_rawProps` parameter, with all member accesses inside the segment body resolving against `_rawProps.<key>`. Includes the parent-side wiring that extracts inline `on*$` handlers into real QRL slots with `q:p: _rawProps`.
**Depends on**: Phase 4 (which clarifies the captures emission path and shares helpers in `src/optimizer/segment-codegen/body-transforms.ts` that this phase extends).
**Requirements**: CONV-05
**Success Criteria** (what must be TRUE):
  1. The 5 currently-failing `_rawProps` tests (`destructure_args_inline_cmp_block_stmt`, `destructure_args_inline_cmp_block_stmt2`, `destructure_args_inline_cmp_expr_stmt`, `example_props_optimization`, `should_wrap_prop_from_destructured_array`) move from failing to passing in `pnpm vitest convergence`.
  2. For `({ data }) => ...`, `({...props}) => { const { data } = props; ... }`, `({ count, some=1+2, hello=CONST, stuff: hey, ...rest }) => ...`, and array-destructured prop bindings, the parent's component arrow takes `(_rawProps)` and every cross-boundary access resolves against `_rawProps.<key>` (or `_rawProps[i]` for arrays).
  3. The 178 currently-passing convergence tests still pass — no regression in `pnpm vitest convergence`.
  4. The change is localized to `src/optimizer/rewrite/raw-props.ts`, `src/optimizer/utils/props-field-rewrite.ts`, `src/optimizer/segment-codegen/body-transforms.ts`, and `src/optimizer/rewrite/const-propagation.ts` (alias resolution); other modules unchanged.
**Plans**: TBD

### Phase 6: Const propagation and folding across the segment boundary
**Goal**: TS optimizer inlines parent-scope literal `const` initialisers into the single segment that references them (deleting the parent declaration), folds compile-time JSX-prop conditionals (`("true" + 1 ? "true" : "")` → `'true'`), flattens nested member-access destructure chains, and stops emitting `_auto_` re-exports for parent-only-referenced module-level decls. Includes the one-line `is_const` parity fix at `src/optimizer/transform/jsx.ts:228-234`.
**Depends on**: Phase 5 (so the props-rewrite shape is settled — const-propagation operates on the post-rewrite parent module and the `_rawProps`-aware segment bodies).
**Requirements**: CONV-06
**Success Criteria** (what must be TRUE):
  1. The 7 currently-failing const-propagation tests (`example_invalid_segment_expr1`, `example_capture_imports`, `example_getter_generation`, `example_use_optimization`, `example_invalid_references`, `example_self_referential_component_migration`, `fun_with_scopes`) move from failing to passing in `pnpm vitest convergence`.
  2. For a parent containing `const style = ` `${css1}${css2}` `; useStyles$(style);` the emitted parent body no longer contains the `const style` line and the corresponding segment body inlines the template-literal initialiser; spurious `export { I2 as _auto_I2 }` re-exports for parent-only-referenced bindings are absent.
  3. The CONCERNS TD-11 `is_const` parity fix is applied: all member expressions return `'var'` from `classifyConstness`, regardless of whether the object is an imported identifier (the import-aware branch at `src/optimizer/transform/jsx.ts:228-234` is removed).
  4. The 178 currently-passing convergence tests still pass — no regression in `pnpm vitest convergence`.
  5. The change is localized to `src/optimizer/rewrite/const-propagation.ts`, `src/optimizer/const-replacement.ts`, `src/optimizer/transform/jsx.ts`, and `src/optimizer/variable-migration.ts`; other modules unchanged.
**Plans**: TBD

### Phase 7: Residual one-offs sweep
**Goal**: TS optimizer closes the residual convergence failures that don't fall into clusters 1-6 — segment-metadata field drift (`parent`, `ctxName`, `loc`), `@jsxImportSource react` directive handling, `mode: 'lib'` inlining, prod-mode symbol re-naming for already-inlined `inlinedQrl`s, dev-mode QRL emission, `regCtxName` hoist strategy, and self-referential QRL inline path quirks. Re-run the suite at phase entry: the failing count entering this phase is whatever survives the cascade from phases 1-6 (likely smaller than the 9 listed in the triage).
**Depends on**: Phase 6 (this phase is deliberately last — the triage notes several Cluster 7 entries close as side-effects of clusters 1-6, so re-baseline before working).
**Requirements**: CONV-07
**Success Criteria** (what must be TRUE):
  1. `pnpm vitest convergence` reports 212/212 passing on completion of this phase. No failing tests remain.
  2. All residual deltas listed in CONVERGENCE-TRIAGE Cluster 7 are either closed or explicitly attributed to a side-effect closure from phases 1-6 (with a note in the phase's plan).
  3. The 178 originally-passing convergence tests, plus all tests closed by phases 1-6, still pass — no regression at any point.
  4. Changes are scoped to the file list per Cluster 7 (`src/optimizer/transform/segment-generation.ts` for metadata drift, `src/optimizer/extract.ts:216` for `@jsxImportSource`, `src/optimizer/inline-strategy.ts` for `mode: 'lib'` and existing `inlinedQrl` parsing, `src/optimizer/dev-mode.ts` for dev QRL emission); each one-off receives the smallest possible change and is documented in its plan.
**Plans**: TBD

## Progress Table

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Path normalization parity | 0/6 | Planned | - |
| 2. Stripped-segment placeholder emission | 0/0 | Not started | - |
| 3. Identifier-collision rewriting for injected imports | 0/0 | Not started | - |
| 4. Captures-vs-paramNames classification | 0/0 | Not started | - |
| 5. `_rawProps` rewriting for destructured/aliased props | 0/0 | Not started | - |
| 6. Const propagation and folding across the segment boundary | 0/0 | Not started | - |
| 7. Residual one-offs sweep | 0/0 | Not started | - |

## Coverage Verification

| Requirement | Phase | Cluster Tests | Notes |
|-------------|-------|---------------|-------|
| CONV-01 | Phase 1 | 4 | Path normalization. Isolated to `path-utils.ts`. |
| CONV-02 | Phase 2 | 3 | Stripped-segment placeholders. Emission concern. |
| CONV-03 | Phase 3 | 1 | Identifier collisions. Smallest cluster. |
| CONV-04 | Phase 4 | 5 | Captures classification. Closes active A1 regression. |
| CONV-05 | Phase 5 | 5 | `_rawProps` destructure rewriting. |
| CONV-06 | Phase 6 | 7 | Const propagation/folding. Largest/most structural. |
| CONV-07 | Phase 7 | 9 | Residual sweep. Count likely shrinks after 1-6. |

**Coverage:** 7/7 v1 requirements mapped (no orphans, no duplicates).
**Cluster total:** 4 + 3 + 1 + 5 + 5 + 7 + 9 = 34 = currently-failing test count.

## Out-of-Scope (Explicit)

The following must NOT appear in any phase per `.planning/PROJECT.md` and `.planning/REQUIREMENTS.md`:
- Source maps — deferred. magic-string can emit them later when needed.
- Performance optimization / benchmark tuning beyond "don't get slower." Benchmarks remain gated by `QWIK_SWC_BINDING`.
- Features beyond SWC parity — anything the Rust optimizer doesn't already do.
- Doc / CHANGELOG / release polish — revisit in a future milestone.

## Operating Mode

`config.mode = "interactive"`. User reviews diffs in-session before commit on every phase.
Reference Rust source under `./swc-reference-only/` is read-only ground-truth for any semantic question.
North-star command: `pnpm vitest convergence`.

---
*Roadmap created: 2026-05-05*
*Phase 1 planned: 2026-05-06 — 6 plans (01-01 through 01-06).*

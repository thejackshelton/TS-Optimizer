# Requirements: Qwik Optimizer (TypeScript)

**Defined:** 2026-05-05
**Core Value:** AST-semantic equivalence with the SWC reference optimizer. Convergence tests prove this by parsing both outputs with oxc-parser and deep-equal'ing the normalized ASTs. QRL symbol hashes must equal those produced by the original Rust/SWC optimizer.

## v1 Requirements

Drive `pnpm vitest convergence` from 178/212 passing to 212/212 passing,
without regressing any of the 178 currently-passing tests. Each
requirement corresponds to one cluster from
`.planning/research/CONVERGENCE-TRIAGE.md` and maps to one roadmap phase.

### Convergence

- [ ] **CONV-01**: TS optimizer normalizes input file paths consistently with the Rust SWC reference for `../node_modules/...` and subdirectory paths, so AST output matches `match-these-snaps/` for the 4 currently-failing path-normalization tests
- [ ] **CONV-02**: TS optimizer emits placeholder `export const X = null` for stripped segments instead of skipping emission entirely, restoring AST match for the 3 currently-failing stripped-placeholder tests
- [ ] **CONV-03**: TS optimizer rewrites identifiers when user code collides with names of injected QRL imports, restoring AST match for the `example_qwik_conflict` test
- [ ] **CONV-04**: TS optimizer correctly classifies captures vs. paramNames for non-`component$` lightweight functional components — closes the active yuku/A1 regression at `src/optimizer/extract.ts:583-590` and restores AST match for the 5 currently-failing classification tests
- [ ] **CONV-05**: TS optimizer correctly rewrites `_rawProps` for destructured and aliased component arguments in `src/optimizer/rewrite/raw-props.ts` and `src/optimizer/segment-codegen/body-transforms.ts`, restoring AST match for the 5 currently-failing `_rawProps` tests
- [ ] **CONV-06**: TS optimizer propagates parent-scope literal `const`s into segment bodies and folds compile-time conditionals (matching SWC's inlining behavior), restoring AST match for the 7 currently-failing const-propagation tests
- [ ] **CONV-07**: TS optimizer closes the residual 9 one-off convergence failures that don't fall into clusters CONV-01..06 (revisited after CONV-01..06 land — count may shrink as upstream fixes cascade)

## v2 Requirements

(None — anything not in v1 is explicitly Out of Scope for this milestone.)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Source maps | Deferred. magic-string can emit them when needed; not required for SWC parity. |
| Performance optimization / benchmark tuning | No perf goal beyond "don't get slower." Benchmarks remain gated by `QWIK_SWC_BINDING`. |
| Features beyond SWC parity | This milestone is about reaching parity, not exceeding it. Anything the Rust optimizer doesn't already do is excluded. |
| Doc / CHANGELOG / release polish | Not blocking convergence; revisit in a future milestone if/when publishing becomes a concern. |

## Traceability

Populated by `gsd-roadmapper` on 2026-05-05.

| Requirement | Phase | Status |
|-------------|-------|--------|
| CONV-01 | Phase 1 — Path normalization parity | Pending |
| CONV-02 | Phase 2 — Stripped-segment placeholder emission | Pending |
| CONV-03 | Phase 3 — Identifier-collision rewriting for injected imports | Pending |
| CONV-04 | Phase 4 — Captures-vs-paramNames classification | Pending |
| CONV-05 | Phase 5 — `_rawProps` rewriting for destructured/aliased props | Pending |
| CONV-06 | Phase 6 — Const propagation and folding across the segment boundary | Pending |
| CONV-07 | Phase 7 — Residual one-offs sweep | Pending |

**Coverage:**
- v1 requirements: 7 total
- Mapped to phases: 7 (every CONV-NN → exactly one Phase N; no orphans, no duplicates)
- Unmapped: 0

---
*Requirements defined: 2026-05-05*
*Last updated: 2026-05-05 — traceability populated by gsd-roadmapper*

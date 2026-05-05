# Qwik Optimizer (TypeScript)

## What This Is

A drop-in TypeScript replacement for Qwik's Rust/SWC optimizer. It takes
Qwik source files containing `$()` boundaries, extracts lazy-loadable
segments, computes captures, generates QRLs, and emits transformed output
that is AST-semantically equivalent to what Qwik's existing SWC optimizer
produces, so QRL hashes resolve correctly and existing Qwik apps run
unchanged. Consumed by Qwik core's existing Vite plugin via the same
NAPI function signature.

## Core Value

AST-semantic equivalence with the SWC reference optimizer. Whitespace,
import ordering, and other formatting differences don't count. What
matters is that the parsed AST is semantically equivalent — that's what
guarantees correct Qwik runtime behavior at hydration/lazy-load time.
Convergence tests prove this by parsing both outputs with oxc-parser
and deep-equal'ing the normalized ASTs.

## Requirements

### Validated

<!-- In-code, green in convergence: 178 of 212 tests passing as of 2026-05-05. -->

- ✓ Working parse → walk → segment-codegen → emit pipeline (oxc-based)
- ✓ Transform-session-driven rewrite architecture (`src/optimizer/utils/transform-session.ts`)
- ✓ Segment extraction with capture analysis for `component$` / `$()` boundaries
- ✓ QRL symbol hashes match those produced by the Rust/SWC optimizer (siphash JS port verified against Rust `DefaultHasher`, SipHash-1-3, keys (0,0))
- ✓ magic-string-based output assembly (no full AST reprint)
- ✓ NAPI-shaped public API (drop-in for `@builder.io/qwik/optimizer`)
- ✓ 178/212 convergence tests producing AST-semantic match against `match-these-snaps/` snapshots

### Active

<!-- This milestone: drive convergence to 212/212 with no regressions in the 178 currently passing. -->

- [ ] **CONV-01**: Path normalization for `../node_modules/...` and subdirectory file inputs (4 tests)
- [ ] **CONV-02**: Stripped-segment placeholder emission (`export const X = null` instead of skip) (3 tests)
- [ ] **CONV-03**: Identifier-collision rewriting between user code and injected QRL imports (1 test)
- [ ] **CONV-04**: Captures-vs-paramNames classification for non-`component$` lightweight functional components — closes the active yuku/A1 regression at `src/optimizer/extract.ts:583-590` (5 tests)
- [ ] **CONV-05**: `_rawProps` rewriting for destructured/aliased component arguments (5 tests)
- [ ] **CONV-06**: Const propagation/folding into segments — inline parent-scope literal `const`s and fold compile-time conditionals (7 tests)
- [ ] **CONV-07**: Residual one-off failures (9 tests; revisit after CONV-01..06 land — likely shrinks as upstream fixes cascade)

### Out of Scope

- Source maps — deferred; magic-string can emit them later when needed.
- Performance optimization / benchmark tuning — no perf goal beyond "don't get slower"; benchmarks remain gated by `QWIK_SWC_BINDING`.
- Features beyond SWC parity — anything the Rust optimizer doesn't already do.

## Context

Brownfield migration of Qwik's Rust/SWC optimizer to a TypeScript
implementation backed by Rust OXC bindings. The reference Rust source is
checked in at `./swc-reference-only/` (read-only) and is the ground truth
for any semantic question. The convergence test suite (`pnpm vitest
convergence`) compares the TS optimizer's emitted snapshots in
`./ts-output/` against expected snapshots in `./match-these-snaps/` taken
from the SWC reference; passing the suite is the north-star metric.

The codebase quality is strong: working transform-session architecture,
2,500+ lines of structured codebase maps in `.planning/codebase/`, and a
523-line failure triage at `.planning/research/CONVERGENCE-TRIAGE.md`
that informed the seven CONV-* requirements above. Most remaining work
is targeted parity-gap closure rather than new construction.

## Constraints

- **API compatibility**: Drop-in for the existing NAPI module — same function signature, same output shape — so Qwik core's Vite plugin consumes it without changes.

- **AST-semantic parity**: Output must be AST-semantically equivalent to what the SWC optimizer produces. Convergence tests parse both expected (`match-these-snaps/`) and actual (`ts-output/`) and compare normalized ASTs — whitespace and import order are ignored by design. Semantic identity is what guarantees correct Qwik runtime behavior.

- **QRL hash parity**: For any given source file, the QRL symbol hashes emitted by the TS optimizer must equal the hashes the original Rust/SWC optimizer would emit for the same input. This requires both the same algorithm (SipHash-1-3 with keys (0,0), matching Rust's `std::collections::hash_map::DefaultHasher`) and byte-identical hash inputs (path normalization, identifier serialization, segment numbering must mirror the Rust impl). A wrong hash breaks QRL resolution in apps already deployed against SWC-generated hashes.

- **No double codebase**: Single TS implementation. The Rust source under `swc-reference-only/` is reference material only; no parallel build pipeline.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Use oxc-parser + oxc-transform + oxc-walker (not Babel) | ~100x faster than Babel; ESTree-conformant; native NAPI; ScopeTracker built-in | ✓ Good |
| Use magic-string for output assembly | Surgical text replacement preserves original formatting; avoids lossy AST reprint; battle-tested in Vite/Rollup | ✓ Good |
| Use siphash JS package for SipHash-1-3 | Authoritative impl by SipHash co-author; verified to match Rust DefaultHasher byte-for-byte | ✓ Good |
| North-star metric is convergence test pass rate | AST-semantic SWC parity is the only proof QRLs resolve correctly at runtime | — Pending (178/212) |
| Phase shape = 7 phases, one per triage cluster (CONV-01..07) | Clusters are coherent failure modes; one-per-phase gives clean success criteria; matches user's preference for clear gates | — Pending |
| Manual diff review before each commit | User reviews diffs in-session before I commit; lighter weight than auto code-review agent | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-05-05 after initialization*

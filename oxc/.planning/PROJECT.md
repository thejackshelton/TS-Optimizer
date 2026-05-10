# Qwik Optimizer (OXC)

## What This Is

A ground-up rewrite of the Qwik Optimizer, migrating from SWC to OXC (Oxide) as the parsing/transform foundation. The optimizer extracts Qrl segments from Qwik components for lazy-loading. The goal is full functional parity with the existing SWC-based optimizer so it can serve as a test-verified drop-in replacement.

## Core Value

All 201 SWC optimizer snapshot cases produce functionally equivalent output from the OXC-based optimizer, with a matching JS-facing API surface.

## Requirements

### Validated

- ~ AST parsing via OXC parser (oxc_parser) — existing
- ~ AST traversal and mutation via oxc_traverse — existing
- ~ Qrl segment extraction from component closures — existing
- ~ Dead code elimination for extracted segments — existing
- ~ Illegal code detection inside Qrl scopes — existing
- ~ Import cleanup and legacy @builder.io rename — existing
- ~ Entry strategy support (Segment, Component, etc.) — existing
- ~ Segment naming with collision detection — existing
- ~ NAPI binding exposing transform_modules to Node.js — existing
- ~ Insta snapshot testing infrastructure — existing
- ~ TransformModulesOptions / TransformOutput / Diagnostic API types — existing

### Active

- [ ] Functional parity with all 201 SWC optimizer snapshot cases
- [ ] Full JS-facing API surface matching the original optimizer (options, diagnostics, segment analysis)
- [ ] All transform edge cases handled (destructuring, default exports, derived signals, inline components, etc.)

### Out of Scope

- Exact byte-for-byte output matching — functional equivalence is sufficient
- Mobile/WASM build targets — focus on Node.js NAPI binding
- Performance benchmarking against SWC — correctness first
- New optimizer features beyond SWC parity — no scope creep

## Context

- The existing SWC-based optimizer lives at [QwikDev/qwik build/v2](https://github.com/QwikDev/qwik/tree/build/v2/packages/optimizer)
- Jack Shelton's OXC conversion is a reference: [thejackshelton/qwik-oxc-optimizer](http://github.com/thejackshelton/qwik-oxc-optimizer)
- Jack's AI-generated spec provides additional context: [qwik-optimizer-spec](https://jack-shelton.hashnode.dev/page/qwik-optimizer-spec)
- Currently 31 tests pass from 15 test input files; 201 SWC expected snapshots define the target
- The codebase uses OXC 0.94.0 across parsing, codegen, traversal, semantic analysis, and minification
- Rust workspace with two crates: `optimizer/` (core) and `napi/` (Node.js binding)

## Constraints

- **Tech stack**: Must use OXC (Oxide) — this is the entire motivation for the rewrite
- **API compatibility**: TransformModulesOptions, TransformOutput, TransformModule, SegmentAnalysis, Diagnostic must match the original optimizer's JS interface (serde camelCase)
- **Testing**: Insta snapshot tests are the primary verification mechanism; SWC expected snapshots under `optimizer/src/snapshots/swc_expected/` define acceptance criteria

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| OXC over SWC | OXC is faster, better maintained, and has a more ergonomic Rust API | -- Pending |
| Functional parity, not byte-identical | Output format differences (whitespace, import ordering) are acceptable as long as behavior matches | -- Pending |
| Insta snapshot testing | Deterministic, reviewable output verification | ~ Good |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? -> Move to Out of Scope with reason
2. Requirements validated? -> Move to Validated with phase reference
3. New requirements emerged? -> Add to Active
4. Decisions to log? -> Add to Key Decisions
5. "What This Is" still accurate? -> Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-04-08 after initialization*

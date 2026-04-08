# Codebase Concerns

**Analysis Date:** 2026-04-08

## Unimplemented Entry Strategies

**Component and Smart Strategy Panics:**
- Issue: `PerComponentStrategy` and `SmartStrategy` explicitly panic with "Not implemented" instead of returning a value
- Files: `optimizer/src/entry_strategy.rs` (lines 73, 94)
- Impact: Using `EntryStrategy::Component` or `EntryStrategy::Smart` will crash at runtime
- Fix approach: Implement the logic based on comments in the code, following the original Qwik optimizer's behavior for these strategies. Uncomment and complete the implementations shown in comments.

## Debug Output Enabled in Production

**Debug Logging Statements:**
- Issue: Multiple `println!()` calls left throughout transformation pipeline
- Files: `optimizer/src/transform.rs` (lines 267, 271, 319, 399, etc.)
- Impact: Generates noise in stdout during normal optimization, performance degradation
- Fix approach: Remove or gate debug output behind a proper logging framework (e.g., tracing crate), or compile out as debug-only with `#[cfg(debug_assertions)]`

**Debug Constants:**
- Issue: `const DEBUG: bool = true` and `const DUMP_FINAL_AST: bool` allow dumping entire ASTs
- Files: `optimizer/src/transform.rs` (lines 262, 263)
- Impact: Excessive console output, potential security concerns if sensitive code is dumped
- Fix approach: Move to environment variable or build-time feature flags, never enable in release builds

## Test Coverage Gaps

**Missing Snapshot Tests:**
- Issue: Only ~13 active test functions exist, but 201 expected snapshot files exist in `swc_expected/` directory
- Files: `optimizer/src/snapshots/swc_expected/` (201 files), test functions in `optimizer/src/js_lib_interface.rs`
- Impact: Large portion of expected transform outputs are untested, undetected regressions possible
- Fix approach: Either generate test functions from the snapshot files or explicitly test all cases in `swc_expected/` directory. The acceptance criteria mandates compatibility with all these cases.

**Disabled Tests:**
- Issue: `test_example_9` and `test_example_10` are disabled with comments indicating known failures
- Files: `optimizer/src/js_lib_interface.rs` (lines 374-389)
- Impact: Known bugs are silently skipped; no visibility on progress toward fixing
- Fix approach: File explicit issues for each disabled test with reproduction steps and expected behavior

**Incomplete Error Assertion:**
- Issue: `test_example_capturing_fn_class` has assertion logic commented out
- Files: `optimizer/src/js_lib_interface.rs` (lines 405-425)
- Impact: Illegal code detection not verified to work correctly
- Fix approach: Uncomment assertions and verify error detection for illegal function/class declarations in QRL scopes

## Fragile Transform Pipeline

**Mutable AST Traversal with Stack-Based State:**
- Issue: Complex state management via nested stacks (`segment_stack`, `import_stack`, `const_stack`, `qrl_stack`, `jsx_stack`, `expr_is_const_stack`) during mutation
- Files: `optimizer/src/transform.rs` (lines 130-166)
- Impact: Easy to push/pop items in wrong order, causing crashes or silent state corruption. Hard to reason about correct state at each step
- Fix approach: Use structured result-based AST transformation where state is immutable/scoped, or add assertions to validate stack invariants at critical points

**replace_expr Field:**
- Issue: `replace_expr: Option<Expression>` used as a temporary holder for expression replacements, mutated during exit handlers
- Files: `optimizer/src/transform.rs` (line 166)
- Impact: Non-obvious side-channel for AST mutations; difficult to track when replacements are consumed; potential for use-after-free if not properly consumed
- Fix approach: Make replacement explicit in the traversal API or use a scope-based transaction pattern

## Incomplete Feature Implementation

**JSX Transpilation Partially Implemented:**
- Issue: JSX transpilation code exists but with large commented sections and placeholder logic
- Files: `optimizer/src/transform.rs` (JSX-related enter/exit handlers)
- Impact: JSX optimization may not function correctly for complex components
- Fix approach: Review against original Qwik optimizer JSX handling and complete implementation

**ImportCleanUp Module:**
- Issue: Module exists but minimal implementation; may not correctly remove unused imports introduced during transformation
- Files: `optimizer/src/import_clean_up.rs`
- Impact: Extracted modules may have stale imports, increasing bundle size
- Fix approach: Verify against original optimizer's import cleanup logic and test with real components

## Missing Type Safety

**allow(unused) Attribute:**
- Issue: `#![allow(unused)]` at top of transform.rs suppresses warnings about unused code/imports
- Files: `optimizer/src/transform.rs` (line 1)
- Impact: Real dead code or incorrect imports hidden from compiler
- Fix approach: Remove allow(unused) and clean up actual unused code/imports. If some are intentional WIPs, add specific `#[allow(unused)]` on those items only

## Error Handling Gaps

**Minimal Diagnostic Information:**
- Issue: `ProcessingFailure` enum only contains `IllegalCode` variant; no other error types
- Files: `optimizer/src/processing_failure.rs`
- Impact: Parser errors, semantic errors, and other failures during transform are logged but not exposed to the JS interface as structured diagnostics
- Fix approach: Extend `ProcessingFailure` with variants for other error categories and propagate parse/semantic errors from the pipeline

**Silent Error Suppression:**
- Issue: Parse and semantic errors collected in `errors` vectors but not included in diagnostics output
- Files: `optimizer/src/transform.rs` (lines 1147, 1171)
- Impact: Build-breaking issues silently ignored, no feedback to developers
- Fix approach: Convert all collected errors to `ProcessingFailure` and include in final output

## Performance Considerations

**Cloning Entire Components:**
- Issue: `self.components.clone()` in OptimizedApp assignment and multiple component clones throughout pipeline
- Files: `optimizer/src/transform.rs` (line 290)
- Impact: Unnecessary memory allocation and copying of component data structures
- Fix approach: Use reference counting or move semantics instead of clone

**Symbol/Reference Lookup in HashMaps:**
- Issue: Multiple HashMap lookups and iterations for symbol resolution and import tracking
- Files: `optimizer/src/transform.rs` (lines 134, 144)
- Impact: O(n) lookups during traversal; slow for large modules with many symbols
- Fix approach: Profile against real-world components; consider using OXC's built-in symbol resolution

## Known Limitations

**No Source Map Generation:**
- Issue: Source maps are marked as optional but infrastructure for generating them is incomplete
- Files: `optimizer/src/js_lib_interface.rs` (lines 94)
- Impact: Cannot debug transformed code back to original source
- Fix approach: Implement source map generation using OXC's codegen capabilities

**Limited Entry Strategy Support:**
- Issue: Only Inline, Hoist, Single, Hook, and Segment strategies are functional; Component and Smart not implemented
- Files: `optimizer/src/entry_strategy.rs`
- Impact: Developers cannot use all entry strategies available in Qwik optimizer
- Fix approach: Implement Component and Smart strategies (currently panicking)

## Acceptance Criteria Risk

**Major Test Coverage Shortfall:**
- Risk: Mandatory acceptance criteria requires "code base must correctly transform code to match all cases defined by `*.snap` files under `optimizer/src/snapshots/swc_expected`"
- Files: ACCEPTANCE.md in project, `optimizer/src/snapshots/swc_expected/` (201 files)
- Current state: Only ~13 of 201 expected test cases are actively tested
- Impact: High risk of undetected regressions and incomplete migration from SWC
- Fix approach: Create comprehensive test suite covering all 201 snapshot files; prioritize fixing disabled tests and integrating swc_expected snapshots into test harness

## Dependencies

**OXC Version Lock:**
- Issue: All OXC crates pinned to exact version (0.94.0)
- Files: `optimizer/Cargo.toml` (lines 10-20)
- Impact: Cannot upgrade OXC without updating all crates simultaneously; security patches may lag
- Fix approach: Use semantic versioning (0.94.x) to allow patch updates within minor version

---

*Concerns audit: 2026-04-08*

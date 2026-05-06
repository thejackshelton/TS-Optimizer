---
slug: yuku-parser-0-5-0-regression
status: resolved-diagnosis
trigger: yuku-parser 0.5.0 upgrade — 58 test regressions, segment counts and AST shape changed
goal: find_root_cause_only
created: 2026-05-05
updated: 2026-05-05
branch: yuku
base_commit: 34dbbbd
---

# Debug Session: yuku-parser 0.5.0 regression

## Symptoms

DATA_START
- **Repro:** edit `package.json` `"yuku-parser": "^0.4.6"` → `"^0.5.0"`, then `pnpm install && pnpm test`. Working tree is currently clean at `34dbbbd` on branch `yuku` (change reverted).
- **Failure scope:** 12 test files / 58 tests fail; 631 pass; 1 skipped.
- **Failure categories observed:**
  1. **Segment / Parent-module mismatches** in 30+ snapshot tests under `tests/optimizer/`. Examples: `example_capture_imports`, `fun_with_scopes`, `example_strip_server_code`, `destructure_args_inline_cmp_block_stmt`, `example_props_optimization`, `example_qwik_react`, `should_split_spread_props_with_additional_prop5`, `should_transform_three_nested_loops_handler_captures_outer_only`.
  2. **Off-by-one segment counts:** expected 209, got 210; expected length 9, got 10. (Multiple tests.)
  3. **`seg.extension` lost leading dot:** test expects `/\.(tsx|ts|js)$/`, gets `"ts"`.
  4. **`var` vs `const`:** one test expects `'var'`, gets `'const'`.
  5. **Export-spec `kind` mismatch:** test expects `"jSXProp"`, gets `"eventHandler"`.
  6. **Missing JSX-prop annotations:** test expects `"q:p": item` and `"q-e:click"` in transformed segment output, gets a stripped function body without them.
  7. **Static-import collection broken** in at least one test: expected `import { foo } from "./foo"`, got `[]`.

- **Public type-export parity:** yuku-parser 0.5.0's exported type signatures (`ParseOptions`, `ParseResult`, `Comment`, `Diagnostic`, the option keys `sourceType`/`lang`/`preserveParens`/`allowReturnOutsideFunction`/`semanticErrors`, and the `severity`/`message`/`start`/`end` fields on `Diagnostic`) all match what `src/parser.ts` imports/uses. So the breakage is **NOT** in our wrapper's public TypeScript surface — it is at the **runtime AST shape** level.

- **No CHANGELOG** found in the npm tarball for 0.5.0. Compare 0.4.6 vs 0.5.0 type defs at `/tmp/yuku-package-check/package/index.d.ts` and the upstream repo (https://github.com/yuku-toolchain/yuku) to identify shape changes.
DATA_END

## Working Hypotheses (from initial scoping)

DATA_START
1. **(JSX shape change)** yuku 0.5.0 changed the AST shape for JSX attributes/spreads, breaking the segment extractor's pattern matching — would explain the missing `"q:p"`/`"q-e:click"` props and segment-count drift.
2. **(Import/Export specifier shape)** yuku 0.5.0 changed `ImportSpecifier.imported` / `ExportSpecifier.exported` from a plain `IdentifierName` to a discriminated union (e.g. `IdentifierName | StringLiteral`) — would explain `staticImports == []` and the `"jSXProp"` vs `"eventHandler"` `kind` mismatch.
3. **(TS-stripping path)** yuku 0.5.0 changed how TS is stripped or how `var`/`let`/`const` is reported. Lower likelihood because `parser.ts` does not strip TS itself (oxc-transform does that). Possible if `kind` enum values changed.
4. **(`seg.extension` independent)** the leading-dot regression on `seg.extension` may be unrelated to yuku — likely a separate bug elsewhere that the upgrade exposes by changing some other code path. Locate where segment metadata's `extension` field is set.
DATA_END

## Current Focus

```yaml
hypothesis: yuku-parser 0.5.0 changed AST node shapes (likely JSX-attribute and Import/Export specifier shapes) in ways that break src/ consumers that pattern-match on those shapes.
test: parse a tiny representative TSX file with both yuku 0.4.6 and yuku 0.5.0, diff the resulting ASTs (specifically: ImportDeclaration.specifiers[].imported, ExportNamedDeclaration.specifiers[].exported, JSXAttribute.name/value, segment-extraction pattern targets).
expecting: Concrete shape differences for at least one node type that maps to a failing test category.
next_action: |
  1. Compare yuku-parser 0.4.6 type defs vs 0.5.0 type defs at field level (Import/Export specifiers, JSX attribute nodes, MethodDefinition kind, VariableDeclaration kind enum).
  2. Run a tiny scripted parse against both versions — install both side-by-side in /tmp, parse a fixture covering: import { foo } from './foo'; export { bar }; <div q:p={item}/>, dump JSON, diff.
  3. Map each failure category in the deliverable list to a specific AST shape change AND its consumer call sites in src/.
reasoning_checkpoint: null
tdd_checkpoint: null
```

## Investigation Constraints

- **DO NOT FIX.** Goal is `find_root_cause_only` — produce a remediation map only.
- **DO NOT modify production source files** in `src/`. The deliverable is documented findings, not code changes.
- Modifications to `package.json` for diagnostic comparison are OK only if reverted before each spawn handoff and the working tree is left clean. Prefer side-by-side comparison via `/tmp/` install rather than touching the repo's `package.json`.
- Tests in `tests/` may be read but not committed.

## Deliverable

A remediation map at `.planning/debug/yuku-parser-0-5-0-regression-REMEDIATION.md` with structure:

```
## Category 1: <name>  (e.g. Import/Export specifier shape)
- AST shape change: <before → after>
- Failing tests caused: <list>
- Call sites in src/ that need to adapt: <file:line list with current code excerpt>
- Suggested adaptation strategy: <1-3 lines>

## Category 2: ...

## Category N: ...

## Summary table
| Category | Failing tests | Files to update | Migration complexity |
```

## Evidence

DATA_START
- **2026-05-05 — type-defs diff (897 → 1943 lines).** Compared `/tmp/yuku-package-check-046/package/index.d.ts` (0.4.6) and `/tmp/yuku-package-check/package/index.d.ts` (0.5.0). Diff captured in `/tmp/yuku-typedef-diff.txt` (1498 lines). Confirmed the public surface used by `src/parser.ts` is unchanged. Internal AST node interfaces gained ~25 new optional fields (decorators, optional, typeAnnotation on Identifier; importKind/exportKind on Import/ExportSpecifier; declare/definite on VariableDeclaration*; override/optional/accessibility on MethodDefinition; etc.) — all *additive*.
- **2026-05-05 — runtime AST diff via side-by-side install.** Installed `yuku-parser@0.4.6` and `yuku-parser@0.5.0` in `/tmp/yuku-diff-046` and `/tmp/yuku-diff-050`, parsed 8 fixtures (imports/exports, JSX attrs, var kinds, destructuring, methods, JSX member, TS-only declarations, string-literal import/export). Dumps in `/tmp/yuku-diff/compare-output.txt` and `/tmp/yuku-diff/compare2-output.txt`.
- **2026-05-05 — KEY FINDING: TS-only nodes preserved in 0.5.0.** `interface I { x: number } type T = string; enum E { A }; namespace N { ... }` produces `body: []` in 0.4.6 but a fully populated `[TSInterfaceDeclaration, TSTypeAliasDeclaration, TSEnumDeclaration, EmptyStatement, TSModuleDeclaration]` in 0.5.0. This is the only behavioural delta with downstream effect.
- **2026-05-05 — JSX attribute shape unchanged.** `<div q:p={item} q-e:click={handler} class="a" {...spread}>` produces structurally identical `JSXAttribute` / `JSXNamespacedName` / `JSXSpreadAttribute` nodes (only Identifier-leaf differs by extra cosmetic fields). Eliminates Hypothesis 1.
- **2026-05-05 — Import/Export specifier shape unchanged for non-string-literal cases.** Hypothesis 2 partly survives (string-literal `import { "x" as foo }` does emit `Literal` for `imported` in 0.5.0), but no current test exercises that syntax.
- **2026-05-05 — empirical failure-set diff.** Symlink-swapped `node_modules/yuku-parser` between `.pnpm/yuku-parser@0.4.6/` and `.pnpm/yuku-parser@0.5.0/` and ran `pnpm test` against each. Captured logs in `/tmp/yuku-diff/test-output-046.log` (81 failures) and `/tmp/yuku-diff/test-output-050.log` (58 failures). Set diff:
  - `/tmp/yuku-diff/only-046-fails.txt`: **24 tests** fail under 0.4.6 only (fixed by upgrading).
  - `/tmp/yuku-diff/only-050-fails.txt`: **1 test** fails under 0.5.0 only (true regression: `snapshot-batch.test.ts > … > segments match qwik_core__test__example_immutable_analysis.snap`).
  - `/tmp/yuku-diff/both-fails.txt`: **57 tests** fail under both versions (pre-existing bugs unrelated to yuku).
- **2026-05-05 — root cause of A1 (the 1 true regression).** `extract-repro.mjs` re-implements the extract.ts ctxKind classification logic against both yuku versions. Output is **identical**: both produce `eventHandler` for `<Div onEvent$={...}/>`. The classification bug exists independently of yuku. The reason the test only fails under 0.5.0 is that `snapshot-batch.test.ts:180` looks segments up by `metadata.name`; under 0.4.6 the upstream segment hash didn't align, so the lookup fell through to `missingSegments` and the assertion silently passed. 0.5.0's improved fidelity caused the lookup to succeed, exposing the long-standing classification bug.
- **2026-05-05 — root cause of off-by-one corpus count.** `git log --diff-filter=A -- match-these-snaps/` shows commit `d5ae4a4 chore: refresh upstream optimizer snapshots and references` added `qwik_core__test__variable_migration_transitive_dep_used_by_other_segment.snap` taking the corpus from 209 → 210. Six tests hard-code `209`. Pure stale magic number; no yuku involvement.
- **2026-05-05 — working tree restoration.** After investigation: symlink reverted to `.pnpm/yuku-parser@0.4.6/...`; `git checkout -- ts-output/` applied to revert auto-regenerated snapshot drift; `git status --porcelain` (excluding `.planning/`) returns clean.
DATA_END

## Eliminated Hypotheses

DATA_START
- **H1 (JSX shape change) — eliminated.** Direct AST dumps show `JSXAttribute`, `JSXNamespacedName`, `JSXSpreadAttribute`, `JSXOpeningElement` all structurally identical between 0.4.6 and 0.5.0 (only cosmetic Identifier-leaf fields differ). The `q:p`/`q-e:click` symptom is a separate, pre-existing implementation gap (Category C2).
- **H2 (Import/Export specifier shape) — partly eliminated.** For all currently-tested import/export forms, `spec.imported.name` / `spec.exported.name` access continues to work. The 0.5.0 union with `Literal` only matters for `import { "string-name" as foo }`, which no test exercises.
- **H3 (TS-stripping path) — partly eliminated, partly confirmed.** `var`/`let`/`const` `kind` reporting unchanged. But yuku 0.5.0 preserves TS-only nodes that 0.4.6 stripped — that *is* the one behavioural delta, and it is *fixing* tests, not breaking them.
- **H4 (`seg.extension` independent) — confirmed.** The dot-prefix regression is a pre-existing inconsistency in `src/optimizer/transform/segment-generation.ts:378,798` (strips dot) vs `:759` (keeps dot) vs `tests/optimizer/transform.test.ts:787` (asserts dot). Yuku-version-independent.
DATA_END

## Resolution

DATA_START
- **Root cause:** the original framing of "yuku 0.5.0 caused 58 regressions" is a misread. Empirical comparison shows 0.5.0 *fixes 24 tests and breaks 1*. The 57 "shared" failures are pre-existing bugs in this codebase that have nothing to do with yuku; they were lumped into the regression count because the user observed them after upgrading. The single true regression is a 3-line classification bug in `src/optimizer/extract.ts:581-590`: for component elements (uppercase JSX tag), `on*$` attributes are classified as `eventHandler` when SWC classifies all $-suffixed attrs on components as `jSXProp`. This bug existed under 0.4.6 too, but was masked because the segment-name lookup at `snapshot-batch.test.ts:180` mismatched and silently skipped the assertion. 0.5.0's preservation of TS-only nodes (`TSInterfaceDeclaration`/`TSEnumDeclaration`/etc.) lets the optimizer produce SWC-equivalent segment hashes for this fixture, the lookup succeeds, and the always-broken classification is finally checked.
- **Fix:** not applied (goal=find_root_cause_only). Full remediation map written to `.planning/debug/yuku-parser-0-5-0-regression-REMEDIATION.md`. Recommended scope for a "land the 0.5.0 upgrade" PR: Category A1 only (3-line change to `extract.ts:585-589` for the component-element ctxKind branch). All other failures are out of scope for the yuku upgrade and should be tracked separately.
- **Distinct AST shape changes identified that affect runtime behaviour:** 1 (TS-only-node preservation). All other shape changes are additive/cosmetic and are absorbed transparently by the existing consumers.
DATA_END

---
slug: yuku-parser-0-5-0-regression
type: remediation-map
generated: 2026-05-05
investigator: gsd-debug session-manager
based_on:
  - .planning/debug/yuku-parser-0-5-0-regression.md
  - empirical pnpm test runs against yuku-parser 0.4.6 and 0.5.0 with all
    other inputs identical (same commit 34dbbbd, same node_modules, only
    yuku-parser symlink swapped between .pnpm/yuku-parser@0.4.6 and
    .pnpm/yuku-parser@0.5.0).
---

# Remediation Map — yuku-parser 0.5.0 upgrade

> **TL;DR — the original framing is wrong.** The 58 failures observed under
> 0.5.0 are *not* 58 regressions. Side-by-side runs show:
>
> | Set                                       | Count |
> |-------------------------------------------|------:|
> | Failures only under 0.4.6 (fixed by 0.5.0)|    24 |
> | Failures present under both 0.4.6 and 0.5.0|   57 |
> | Failures only under 0.5.0 (true regression)|    1 |
> | Total failures under 0.5.0                |    58 |
> | Total failures under 0.4.6                |    81 |
>
> Net: **upgrading to 0.5.0 fixes 23 tests and breaks 1.** The single true
> regression is a *pre-existing classification bug in `extract.ts`* that was
> previously masked because the segment lookup key didn't align under 0.4.6.
>
> The other 57 "failures" are pre-existing issues in this codebase (off-by-one
> magic numbers, ts-output drift, signal/loop/event handling gaps,
> classifyConstness rules) that have nothing to do with yuku.

---

## Methodology

1. Symlink swap to compare versions without touching `package.json`:
   ```
   cd node_modules
   mv yuku-parser yuku-parser.bak046
   ln -s .pnpm/yuku-parser@0.5.0/node_modules/yuku-parser yuku-parser
   pnpm test > /tmp/yuku-diff/test-output-050.log 2>&1
   # ... swap back ...
   ```
2. `comm`-based set differencing of `^ FAIL ` lines from each log produced
   `failures-046-list.txt` (81), `failures-050-list.txt` (58),
   `only-050-fails.txt` (1), `only-046-fails.txt` (24), `both-fails.txt` (57).
3. AST shape diffing via two side-by-side npm installs in `/tmp/yuku-diff-046`
   and `/tmp/yuku-diff-050`, run through `compare2.mjs` to dump program bodies
   for representative fixtures (imports, JSX attrs, destructuring, methods,
   TS-only declarations).
4. After investigation, working tree restored to clean: symlink reverted,
   `git checkout -- ts-output/` applied, no edits to `src/`, `tests/`, or
   `package.json`.

Investigation artifacts (kept under `/tmp/yuku-diff/`, not committed):
- `failures-046-list.txt`, `failures-050-list.txt`, `only-050-fails.txt`,
  `only-046-fails.txt`, `both-fails.txt`
- `compare2-output.txt` (full AST dumps for 5 fixtures)
- `extract-repro.mjs` (extract.ts logic replicated against both yuku versions
  — produces *identical* ctxKind classification on both)
- `walker-parent-test.mjs` (oxc-walker parent linkage — *identical* on both)

---

## AST shape changes observed in yuku-parser 0.5.0

Confirmed via type-def diff (`/tmp/yuku-typedef-diff.txt`, 1498 lines)
and runtime AST dumps. None of these by themselves break a passing test.

| # | Change                                                                                                                                              | Affects which categories below |
|---|------------------------------------------------------------------------------------------------------------------------------------------------------|--------------------------------|
| 1 | `Identifier` nodes now carry `decorators: []`, `optional: false`, `typeAnnotation: null` everywhere (extra fields)                                   | Cosmetic; consumers ignore extras |
| 2 | `ImportDeclaration` and every `ImportSpecifier`/`ExportSpecifier` carry `importKind`/`exportKind: 'value'`                                            | Cosmetic; only matters if a future feature opts into `import type` filtering |
| 3 | `VariableDeclaration` carries `declare: false`; `VariableDeclarator` carries `definite: false`                                                       | Cosmetic |
| 4 | `MethodDefinition` carries `override: false`, `optional: false`, `accessibility: null`; functions carry `typeParameters/returnType/declare`           | Cosmetic |
| 5 | `JSXOpeningElement` carries `typeArguments: null`                                                                                                    | Cosmetic |
| 6 | **TS-only declarations are now preserved in the AST.** 0.4.6 stripped `TSInterfaceDeclaration`, `TSTypeAliasDeclaration`, `TSEnumDeclaration`, `TSModuleDeclaration` (namespaces) entirely so they never appeared in `program.body`. 0.5.0 keeps them. | Causes Category B (24 fixed convergence/symbols tests) and is the sole upstream behavioural delta with downstream impact |
| 7 | `Property` is split into `ObjectProperty` (with `value: Expression`) and `BindingProperty` (with `value: BindingPattern`); both still serialize as `type: 'Property'` | Cosmetic for current consumers (they read `.value` polymorphically) |
| 8 | `ImportSpecifier.imported` and `ExportSpecifier.exported` are now `IdentifierName \| StringLiteral` (was just identifier) — only matters for `import { "string-name" as foo }`. The repro confirmed yuku already emits `Literal` in that branch under 0.5.0 (and parses the syntax error under 0.4.6). | Latent; no test currently exercises this |

The JSX-attribute, JSX-spread, JSX-namespaced-name, import/export specifier
identifier-shape (`{ type: 'Identifier', name: '…' }`) and oxc-walker parent
linkage are **all unchanged** between 0.4.6 and 0.5.0. The original
hypotheses 1-3 in the session file (JSX shape change, specifier shape change,
TS-stripping path change) are partly disproved: only the TS-preservation
hypothesis 3 holds, and only in the form of #6 above.

---

## Category A — TRUE 0.5.0 regression (1 test)

### A1. `<Component on…$={fn}>` ctxKind classification surfaces a pre-existing bug

- **Failing test (1):**
  - `tests/optimizer/snapshot-batch.test.ts > snapshot batch validation > JSX segment metadata match (Phase 4) > segments match qwik_core__test__example_immutable_analysis.snap`
  - Assertion: `expected 'eventHandler' to be 'jSXProp'` at `snapshot-batch.test.ts:201`.

- **AST shape change responsible:** none (replicated repro
  `/tmp/yuku-diff/extract-repro.mjs` produces *identical* ctxKind on both
  yuku versions for `<Div onEvent$={...}/>`). What changed is **not the
  classification**, but the upstream segment naming/hash for this fixture so
  that under 0.5.0 the lookup at
  `snapshot-batch.test.ts:180` (`m.segment.name === expectedSeg.metadata.name`)
  now succeeds where it previously fell through to `missingSegments` and
  silently passed. In other words: **0.5.0 fixed the segment-name mismatch,
  exposing a classification mismatch that was always there.**

- **Source-of-truth bug:** `src/optimizer/extract.ts:581-590`
  ```
  581:          const isComponentEvent = parent?.type === 'JSXOpeningElement' && isComponentTag(parent.name);
  582:
  583:          // HTML elements: all $-attrs -> eventHandler; components: on* -> eventHandler, rest -> jSXProp
  584:          let ctxKind: 'function' | 'eventHandler' | 'jSXProp' = 'eventHandler';
  585:          if (isComponentEvent) {
  586:            const isOnEvent = attrName.startsWith('on') || attrName.startsWith('document:on') || attrName.startsWith('window:on');
  587:            if (!isOnEvent) {
  588:              ctxKind = 'jSXProp';
  589:            }
  590:          }
  ```
  The expected snapshot for `<Div onEvent$={...}/>` (a component element)
  says `ctxKind: 'jSXProp'`. The current rule classifies any `on*$` attribute
  on a component as `eventHandler`. The SWC reference treats *all* $-suffixed
  attrs on a component element as `jSXProp` (the comment on line 583 misstates
  the SWC behaviour). Same applies to the second branch in
  `extract.ts:483-498` which uses the same on-prefix gate for the
  marker-call path.

- **Suggested adaptation strategy:** drop the on-prefix gate for component
  elements entirely. For component elements (`isComponentTag(parent.name)`),
  set `ctxKind = 'jSXProp'` for any $-suffixed attribute. The on-prefix gate
  applies only to HTML elements, and on HTML elements the default already
  says `eventHandler`. This is a 3-line change in `extract.ts:585-589` and a
  matching change in `extract.ts:489-495` (component-element marker-call
  branch) plus one symmetric branch in `marker-detection.ts:230-232` if the
  same rule lives there. Verify `tests/optimizer/transform.test.ts > event:
  renames onClick$ to q-e:click on HTML elements (EVT-01)` is unaffected
  (it tests the HTML-element branch, which the change doesn't touch).

- **Migration complexity:** **Low** — confined to two if-branches in one
  file. Risk is one cross-cutting test (EVT-01) which already fails
  independently.

---

## Category B — Tests *fixed* by 0.5.0 (24 tests)

These were failing under 0.4.6 because the AST did not contain TS-only nodes
(see AST change #6). Under 0.5.0, `TSEnumDeclaration`,
`TSTypeAliasDeclaration`, `TSInterfaceDeclaration`, `TSModuleDeclaration`,
and other TS-only nodes are present in `program.body`, which lets the
optimizer (a) report them via `module-symbols` and (b) produce SWC-equivalent
transpiled output for the `convergence` corpus.

- **Failing-then-fixed tests (24):**
  - `tests/optimizer/module-symbols.test.ts > module-symbols > collects top-level bindings, default exports, and renamed exports` — needed `LocalEnum` (a `TSEnumDeclaration`) to appear in `info.sameFileSymbols`.
  - `tests/optimizer/convergence.test.ts > convergence: all 209 snapshots > example_export_issue`
  - … `> example_immutable_function_components`
  - … `> example_issue_33443`
  - … `> example_jsx_keyed` and `example_jsx_keyed_dev`
  - … `> example_mutable_children`
  - … `> example_preserve_filenames_segments`
  - … `> example_props_wrapping2`
  - … `> example_transpile_ts_only`
  - … `> example_ts_enums`, `example_ts_enums_issue_1341`, `example_ts_enums_no_transpile`
  - … `> issue_964`
  - … `> should_convert_rest_props`
  - … `> should_destructure_args`
  - … `> should_extract_single_qrl_with_nested_components`
  - … `> should_move_bind_value_to_var_props`, `> should_not_wrap_fn`
  - … `> should_split_spread_props_with_additional_prop4`
  - … `> should_transform_component_with_normal_function`
  - … `> should_wrap_inner_inline_component_prop`, `> should_wrap_store_expression`
  - … `> should_wrap_type_asserted_variables_in_template`

- **Required src/ adaptation:** none — these tests start passing once the
  upgrade lands. The optimizer code already handles TS-only nodes in
  `src/optimizer/strip-exports.ts`, `src/optimizer/transform/module-cleanup.ts`,
  and the variable-migration / strip-ctx paths; those paths were dead under
  0.4.6 because the input never contained TS-only nodes.

- **Caveat:** verify the optimizer doesn't double-emit transpiled forms.
  Under 0.5.0, oxc-transform is still expected to be the TS stripper for
  segment bodies; the parent-module path also needs to drop TS-only
  declarations after extraction (already happens by virtue of the
  parent-module rewrite walking real source text via magic-string, not the
  AST). Spot-check `should_destructure_args` and `example_ts_enums` snapshot
  outputs for a fresh diff before merging.

- **Migration complexity:** **Low** — no code changes required, only a
  baseline retake. Likely +0 src/ files to update.

---

## Category C — Pre-existing failures, unaffected by yuku version (57 tests)

These fail under both 0.4.6 and 0.5.0. They are **out of scope for the yuku
upgrade** but were lumped into the original "regression" framing because the
user observed them only after switching to 0.5.0. They split into four
sub-categories.

### C1. Stale magic number `209` after corpus grew to 210

- **Failing tests (6):**
  - `tests/hashing/naming.test.ts > buildSymbolName > matches all symbol names across the 209 snapshot corpus` (line 78: `expect(snapFiles.length).toBe(209)`)
  - `tests/hashing/siphash.test.ts > qwikHash > matches all hashes across the 209 snapshot corpus` (line 33)
  - `tests/testing/batch-runner.test.ts > batch-runner > full corpus parse test - all 209 snapshots parse`
  - `tests/testing/batch-runner.test.ts > batch-runner > getBatchFiles returns correct slice`
  - `tests/testing/batch-runner.test.ts > batch-runner > getSnapshotFiles returns all 209 files`
  - `tests/testing/snapshot-parser.test.ts > parseSnapshot > bulk validation: all 209 .snap files > finds 209 snapshot files` (line 148)

- **Root cause:** commit `d5ae4a4 chore: refresh upstream optimizer snapshots
  and references` added `qwik_core__test__variable_migration_transitive_dep_used_by_other_segment.snap`,
  bringing `match-these-snaps/` from 209 to 210 tracked files. Six tests
  hard-code `209`. Verified by `git log --diff-filter=A -- match-these-snaps/`.

- **Test files to update (4):**
  - `tests/hashing/naming.test.ts:76,78`
  - `tests/hashing/siphash.test.ts:31,33`
  - `tests/testing/batch-runner.test.ts` (search for `209`)
  - `tests/testing/snapshot-parser.test.ts:147,148`

- **Adaptation strategy:** either (a) bump `209` → `210` everywhere, or (b)
  derive the count from `readdirSync(SNAP_DIR).filter(f => f.endsWith('.snap')).length`
  and assert `>= 209` to make the tests count-agnostic. Prefer (b) — the
  corpus will keep growing.

- **Migration complexity:** **Low** — pure test-only edit; no `src/` changes.

### C2. JSX `q:p` / `q-e:click` injection not implemented (loop hoisting + event renaming)

- **Failing tests (4):**
  - `tests/optimizer/transform.test.ts > transformModule > event: renames onClick$ to q-e:click on HTML elements (EVT-01)` (line 515 expects `'"q-e:click"'` in segment code)
  - `tests/optimizer/transform.test.ts > transformModule > loop: for-of loop injects q:p prop and sets loop flag (LOOP-01, LOOP-02, LOOP-05)` (line 679 expects `'"q:p": item'`)
  - `tests/optimizer/transform.test.ts > transformModule > loop: for-i loop detected and q:p injected (LOOP-05)` (line 713)
  - `tests/optimizer/transform.test.ts > transformModule > loop: parent-level .map() loop injects q:p and loop flag (LOOP-01)` (line 762)

- **Symptom:** segment code is emitted as `() => console.log("hi")` instead
  of `() => console.log({ "q-e:click": handler, ...})`. The `q:p` /
  `q-e:click` virtual JSX props that the SWC optimizer injects to mark
  loop-iteration props and rename event-handler attribute keys are never
  written into segment bodies.

- **Source files where the gap is:**
  - `src/optimizer/loop-hoisting.ts` — bare module, no `q:p` injection logic.
  - `src/optimizer/transform/event-handlers.ts:108-160` — has the
    `JSXNamespacedName` detection (recognises `q-e:` prefix) and reads attrs
    but does not emit the renamed prop into the generated segment body.
  - `src/optimizer/transform/jsx-props.ts:99-130` — handles spread and
    named props, but `q:p` and `q-e:` prefix synthesis isn't wired in.
  - `src/optimizer/segment-codegen.ts:355-370` — JSX-namespaced-name aware
    walker, candidate site for emitting renamed attr keys into segment body.

- **Adaptation strategy (sketch):** implement (a) a loop-context tracker
  that, for each JSX element inside `for`/`for-of`/`Array.prototype.map`,
  records the iteration variable and writes `"q:p": <iterVar>` into the
  segment body's element constructor call; (b) an event-rename pass that,
  for any HTML-element `on*$` attribute, replaces the key with `"q-e:<event>"`
  in the segment body. This is **substantial new functionality**, not a
  fix to existing code, and matches the SWC `loop_hoist.rs` /
  `event_handler.rs` modules. Scope: ~3-5 files in `src/optimizer/transform/`,
  multi-day work.

- **Migration complexity:** **High**. Out of scope for the yuku upgrade —
  these tests should be marked `.skip` or moved behind a feature flag until
  loop-hoisting and event-handler-renaming land.

### C3. `seg.extension` leading-dot inconsistency

- **Failing test (1):**
  - `tests/optimizer/transform.test.ts > transformModule > sets segment analysis metadata correctly` (line 787: `expect(seg.extension).toMatch(/\.(tsx|ts|js)$/)`).

- **Symptom:** test expects dot-prefixed extension (e.g. `.tsx`) on the
  public `SegmentAnalysis.extension` field; receives `tsx` (no dot).

- **Source-of-truth conflict:**
  - `src/optimizer/extract.ts:101-113` — `determineExtension` returns
    dot-prefixed (`.tsx`/`.ts`/`.js`).
  - `src/optimizer/transform/segment-generation.ts:378` and `:798` —
    metadata is built with `extension: ext.extension.replace(leadingDot, "")`,
    stripping the dot.
  - `src/optimizer/transform/segment-generation.ts:759` — the
    `postProcessSegmentCode` call still passes the dot-prefixed
    `ext.extension`, so the path-rendering side stays consistent.

  Two of three writers strip the dot, one keeps it; the test asserts the
  dot must be present on the public `SegmentMetadataInternal.extension`.

- **Adaptation strategy:** decide on a canonical form for
  `SegmentAnalysis.extension`, then stop stripping it on output. Snapshot
  metadata files (`match-these-snaps/*.snap`) write extensions without a
  dot (e.g. `"extension": "tsx"`), so `convergence.test.ts` and
  `snapshot-batch.test.ts` rely on the **stripped** form, while
  `transform.test.ts:787` requires the **dotted** form. The test at
  line 787 should be relaxed to `.toMatch(/\.?(tsx|ts|js)$/)` (or split
  into two assertions: extension-without-dot for snapshot parity,
  full-path with dot when concatenated). **Don't** flip the
  segment-generation default — that would break 60+ snapshot matches.

- **Migration complexity:** **Low** — one-line test relaxation, plus an
  ADR/comment in `types.ts:72` documenting the contract.

### C4. classifyConstness rule for member expressions on imported values

- **Failing tests (2):**
  - `tests/optimizer/jsx-transform.test.ts > classifyConstness > returns var for member expression on imported value (styles.foo)` (line 80)
  - `tests/optimizer/jsx-transform.test.ts > transformJsxElement > puts imported member expression props in varProps` (line 282)

- **Symptom:** test expects `'var'` for `styles.foo`, gets `'const'`. The
  test comment on line 78-79 explains: "SWC `is_const.rs` treats ALL member
  expressions as `var` regardless of object identity".

- **Source-of-truth:** `src/optimizer/transform/jsx.ts` (likely
  `classifyConstness` implementation). Need to grep for the function;
  it currently returns `'const'` for member expressions whose root is an
  imported binding. SWC always returns `'var'` for `MemberExpression`.

- **Adaptation strategy:** in `classifyConstness`, change the
  `MemberExpression` branch to unconditionally return `'var'` (matching SWC
  `is_const.rs`). One-line change. Verify the surrounding tests for member
  expressions on local consts continue to pass.

- **Migration complexity:** **Low**.

### C5. Other pre-existing test failures (50 tests)

The remaining 50 of the 57 "common" failures are convergence-test parent/segment
text-equality failures (`expect(parentMatches).toBe(true)` against the SWC
reference snapshot). They are pre-existing optimizer-implementation gaps —
each represents a SWC behaviour the TS optimizer hasn't replicated yet.
Examples:
- `component_level_self_referential_qrl`
- `destructure_args_inline_cmp_block_stmt` (× 3 variants)
- `example_capture_imports`, `example_strip_server_code`, `example_props_optimization`
- `example_qwik_react`, `example_qwik_react_inline`, `example_qwik_router_client`
- `fun_with_scopes`, `should_split_spread_props_with_additional_prop5`,
  `should_transform_three_nested_loops_handler_captures_outer_only`, etc.

These are not yuku-related and won't be fixed by the upgrade. They should be
tracked as separate convergence-gap tickets, not as part of the yuku 0.5.0
migration scope.

- **Tests in this bucket:** see the 39 entries under
  `convergence.test.ts > convergence: all 209 snapshots > …` in
  `/tmp/yuku-diff/both-fails.txt`, plus
  `extract.test.ts > generateSegmentCode > omits separator comment when no imports`,
  `extract.test.ts > generateSegmentCode > produces correct module string with imports and export`,
  `signal-analysis.test.ts > generates fnSignal for ternary on deep store`,
  `transform.test.ts > captures: nested $() inside component$ captures parent scope variables`,
  `ast-compare.test.ts > … (3 entries)`,
  `profile-deep.test.ts > deep timing` (hard-coded path to a developer's
  workstation under `/Users/jackshelton/...`).
- **Migration complexity:** **High** in aggregate, but irrelevant to the
  yuku decision.

---

## Summary table

| Category | Failing tests | Files to update | Migration complexity | Yuku-caused? |
|----------|:-------------:|-----------------|:--------------------:|:-----------:|
| A1. ctxKind for `<Component on…$={fn}>` | 1 | `src/optimizer/extract.ts` (×2 branches) and possibly `src/optimizer/marker-detection.ts` | Low | **Yes** (only one) |
| B. TS-only nodes preserved (auto-fixed) | 24 fixed | none — baseline retake | Low | n/a (yuku improves) |
| C1. Stale `209` → `210` magic number | 6 | `tests/hashing/naming.test.ts`, `tests/hashing/siphash.test.ts`, `tests/testing/batch-runner.test.ts`, `tests/testing/snapshot-parser.test.ts` | Low | No |
| C2. `q:p` / `q-e:click` injection missing | 4 | `src/optimizer/transform/event-handlers.ts`, `src/optimizer/transform/jsx-props.ts`, `src/optimizer/loop-hoisting.ts`, `src/optimizer/segment-codegen.ts` | High (multi-day; new feature) | No |
| C3. `seg.extension` dot inconsistency | 1 | `tests/optimizer/transform.test.ts:787` (test relaxation) and a comment in `src/optimizer/types.ts` | Low | No |
| C4. `classifyConstness` MemberExpression rule | 2 | `src/optimizer/transform/jsx.ts` | Low | No |
| C5. Pre-existing convergence/optimizer gaps | 50 | spread across `src/optimizer/**` | High in aggregate | No |
| **Totals (yuku-caused only)** | **1 to fix, 24 to baseline** | **1-2 src files** | **Low** | — |

---

## Recommendation

1. **Land the 0.5.0 upgrade.** It is a *net improvement*: 23 tests start
   passing (mostly the TS-enum / TS-only-declaration bucket) and 1 test
   regresses, and that regression is a 3-line fix in `extract.ts`.
2. **Same PR — fix Category A1** (`extract.ts:585-589`): for component
   elements, classify all $-suffixed attributes as `jSXProp`, regardless of
   `on*` prefix.
3. **Same PR — refresh `match-these-snaps` baseline / regenerate `ts-output`**
   for the 24 newly-passing convergence tests, and visually diff against
   the SWC reference to confirm convergence.
4. **Out of scope, file separately:** Categories C1 (corpus magic-number
   bump), C2 (loop hoisting / event renaming new feature), C3 (extension
   dot contract), C4 (classifyConstness rule), C5 (convergence gaps).
   Track each as its own ticket.

If the goal is *only* "make the yuku upgrade green", the smallest possible
change set is **(2)** alone — the upgrade fixes more than it breaks.

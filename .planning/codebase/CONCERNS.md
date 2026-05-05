# Codebase Concerns

**Analysis Date:** 2026-05-05

This document catalogs technical debt, fragile areas, parity-with-SWC risks,
hash-stability risks, and correctness pitfalls in the TS Qwik optimizer
(`/Users/scottweaver/Projects/TS-Optimizer`). Concerns are ordered by severity.
For each concern: severity, where it lives, mitigation. The single open active
regression is captured under Active Regressions.

> **Reference:** `.planning/debug/yuku-parser-0-5-0-regression.md` and
> `.planning/debug/yuku-parser-0-5-0-regression-REMEDIATION.md` are an
> already-completed root-cause analysis (resolved-diagnosis status). The
> findings are folded into the items below; no separate re-investigation is
> needed.

---

## Active Regressions

### A1. `<Component on...$={fn}>` mis-classified as `eventHandler` instead of `jSXProp` (CRITICAL — parity bug)

- **Severity:** HIGH (true regression; latent classification bug exposed by
  the yuku/oxc-parser 0.5.0 upgrade because the upgrade fixed segment-name
  alignment so the snapshot lookup at `tests/optimizer/snapshot-batch.test.ts:180`
  now succeeds where it previously fell through to `missingSegments`).
- **Where it lives:** `src/optimizer/extract.ts:583-590` (the
  JSX-attribute-extraction branch). The parallel marker-call branch at
  `src/optimizer/extract.ts:489-495` was already corrected, so the bug is
  asymmetric across the two extraction paths.
- **What is wrong:**
  ```
  584:          let ctxKind: 'function' | 'eventHandler' | 'jSXProp' = 'eventHandler';
  585:          if (isComponentEvent) {
  586:            const isOnEvent = attrName.startsWith('on') || attrName.startsWith('document:on') || attrName.startsWith('window:on');
  587:            if (!isOnEvent) {
  588:              ctxKind = 'jSXProp';
  589:            }
  590:          }
  ```
  For component elements, this still classifies `on*$` attributes as
  `eventHandler`. SWC's `is_const.rs`/`transform.rs` treat **all** $-suffixed
  attributes on a component element as `jSXProp` regardless of `on*` prefix.
- **Failing test:** `tests/optimizer/snapshot-batch.test.ts > snapshot batch
  validation > JSX segment metadata match (Phase 4) > segments match
  qwik_core__test__example_immutable_analysis.snap` — assertion
  `expected 'eventHandler' to be 'jSXProp'` at
  `tests/optimizer/snapshot-batch.test.ts:201`.
- **Mitigation (3-line change):** drop the on-prefix gate in the branch at
  `extract.ts:585-589`. Replace with `if (isComponentEvent) { ctxKind = 'jSXProp'; }`.
  HTML elements still get `eventHandler` from the default. The marker-call
  branch (`extract.ts:489-495`) already follows this rule and can serve as
  the model.
- **Owner suggestion:** whoever lands the yuku-parser 0.5.0 (now `oxc-parser`)
  upgrade — same PR.

---

## Tech Debt

### TD-1. `ts-output/` snapshots are auto-regenerated AND committed to git

- **Severity:** HIGH
- **Files:** `tests/optimizer/convergence.test.ts:100`
  (`writeFileSync(join(TS_OUTPUT_DIR, snapFile), formatSnapshot(...))`),
  the `ts-output/` directory (209 `.snap` files), and `.gitignore` (which
  does *not* exclude `ts-output/`).
- **Issue:** Every `pnpm test` overwrites `ts-output/*.snap` with whatever the
  current optimizer produces. Because `ts-output/` is tracked in git, a
  developer who runs the suite picks up incidental "drift" (cosmetic format
  deltas, ordering deltas, env-dependent output) into their working tree and
  may commit it. Conversely, a buggy patch can silently update the committed
  reference output.
- **Impact:** Code review becomes noisy (reviewers must distinguish drift
  from intent), regressions can slip in through "rubber-stamp" snapshot
  updates, and `git checkout -- ts-output/` is required after exploratory
  test runs (the regression doc explicitly notes this in its "working tree
  restoration" step).
- **Mitigation:** Either (a) add `ts-output/` to `.gitignore` and treat it as
  build output, or (b) gate the write behind an opt-in env var
  (`UPDATE_TS_OUTPUT=1`) so default `pnpm test` does not mutate tracked files.
  Option (a) preferred — `match-these-snaps/` is the source of truth, not
  `ts-output/`.
- **Owner suggestion:** infra/tooling.

### TD-2. `convergence.test.ts` runs a 3,206-line normalization over both expected and actual ASTs before comparing

- **Severity:** HIGH (parity-erosion risk)
- **File:** `src/testing/ast-compare.ts` (3,206 lines, 70+ top-level
  normalization functions named in `normalizeProgram` at
  `src/testing/ast-compare.ts:92-174`).
- **Issue:** The convergence comparator equates ASTs only after running
  ~30 transforms on both sides — `stripQpProperties`, `stripDotWCalls`,
  `stripDotSBodies`, `stripCapturesDeclarations`, `stripIsServerGuards`,
  `stripUnusedImports`, `stripUnusedLocalDeclarations`,
  `stripUnusedModuleLevelDeclarations`, `stripOrphanedSideEffectCalls`,
  `inlineFnSignalSimple`, `canonicalizeQrlVarNames`, `inlineDestructuredBindings`,
  `expandRawPropsCaptures`, `mergeJsxSplitProps`, `unwrapWrapProp`,
  `mergeGetVarConstProps`, etc.
- **Specific masking risks:**
  - `stripQpProperties` (`src/testing/ast-compare.ts:1230-1247`) **strips
    `q:p` and `q:ps` ObjectExpression properties from both sides before
    comparison.** This means convergence cannot see the missing `q:p`
    injection from Category C2 below — convergence tests are blind to the
    loop-hoisting feature gap.
  - `stripIsServerGuards` (`:2109`), `stripDotSBodies` (`:2577`),
    `stripCapturesDeclarations` (`:3141`) erase entire constructs.
- **Impact:** "Pass" on a convergence test does not equal "behaviourally
  identical to SWC". The aggressive normalizer trades parity-fidelity for a
  greener dashboard. Every new normalizer should be an alarm.
- **Mitigation:** (a) audit each normalizer with a comment justifying *why*
  the difference is semantically irrelevant; (b) split the normalizer into
  "always-safe" (sort imports, unwrap single-statement blocks) vs
  "feature-gated" (`stripQpProperties` should only run when both sides are
  expected to lack q:p — otherwise the diff should fail); (c) add a strict
  comparison mode that runs zero normalizations, gated by env var, used by
  CI for at least the must-converge subset.
- **Owner suggestion:** test-infra owner.

### TD-3. `convergence.test.ts` is documented as a "measurement tool, not a gate"

- **Severity:** MEDIUM
- **Files:** `README.md:80-84` ("The convergence tests are a **measurement
  tool**, not a gate. Not all 209 tests pass yet."),
  `tests/optimizer/convergence.test.ts:172-174` (assertions are still
  `expect(...).toBe(true)` so they do fail individually).
- **Issue:** The README and test naming framing actively encourages tolerance
  for failures, but the actual assertions in `convergence.test.ts:173-174`
  are hard. The "measurement tool" framing collides with `pnpm test` exiting
  non-zero. Developers regularly see a sea of red and learn to ignore it,
  which masks new regressions in the noise (Category C5 of the regression
  remediation lists 50 long-standing failures).
- **Mitigation:** either (a) move convergence to a separate `pnpm test:convergence`
  script that does not block CI, with a known-failures allowlist that shrinks
  over time; or (b) wrap each `it(testName)` in a `it.fails(...)` for the
  known-failing set so green = green and red = new regression.
- **Owner suggestion:** test-infra owner.

### TD-4. Source-map support is fully deferred — every emitted module has `map: null`

- **Severity:** MEDIUM
- **Files (every TransformModule emission site):**
  - `src/optimizer/transform/index.ts:518` (`map: null` for the parent module)
  - `src/optimizer/transform/segment-generation.ts:392` (segment via inline strategy)
  - `src/optimizer/transform/segment-generation.ts:812` (segment via default strategy)
  - `src/optimizer/transform/module-cleanup.ts:534` (passthrough module)
- **Issue:** `CLAUDE.md` notes source-map support is deferred, but
  `src/optimizer/types.ts:21` exposes `sourceMaps?: boolean` as a public option
  and the benchmark passes `sourceMaps: false` everywhere. Downstream
  consumers (Qwik Vite plugin) cannot opt into source maps even when they
  ask. There is no enforcement that the option is rejected when `true`.
- **Impact:** Production debugging of optimized output will be impossible;
  stack traces from segments will be unmappable. Drop-in replacement with
  the SWC optimizer is incomplete on this axis.
- **Mitigation:** either (a) reject `sourceMaps: true` with a runtime error
  to prevent silent footgun; or (b) plumb magic-string's
  `generateMap()` through each emission site (4 call sites). Option (a)
  short-term, (b) is the proper fix and `magic-string` already supports it.
- **Owner suggestion:** project owner / planner.

### TD-5. Twelve-plus reparse sites violate the DESIGN.md "parse once" intent

- **Severity:** MEDIUM (perf + correctness)
- **DESIGN.md:** "We should try to avoid cases where we have to reparse... we
  should hold all the AST information and only need to parse once."
- **Reparse sites in `src/`:**
  - `src/optimizer/rewrite/index.ts:169` — re-parses parent module if
    `existingProgram` not passed
  - `src/optimizer/rewrite/const-propagation.ts:27,66,201` — three reparses
    with synthetic filenames (`__rl__.tsx`, `__ic__.tsx`, `__pb__.tsx`)
  - `src/optimizer/rewrite/inline-body.ts:246` — `__body__.tsx`
  - `src/optimizer/segment-codegen.ts:289`,
    `src/optimizer/segment-codegen/import-collection.ts:37`,
    `src/optimizer/segment-codegen/body-transforms.ts:368` — segment-body
    reparse with `(${bodyText})` wrapping
  - `src/optimizer/transform/module-cleanup.ts:69,121,189` — three reparses
    of intermediate forms
  - `src/optimizer/transform/diagnostic-detection.ts:57` — reparses enclosing
    extraction body
  - `src/optimizer/transform/index.ts:163` — parses every extraction body
    independently
  - `src/optimizer/utils/transform-session.ts:33` — wrap-and-reparse helper
    used by 4 callers
- **Issue:** Each reparse is O(n) in source size and additionally re-allocates
  AST node memory. With ~12 reparse sites and recursive use during segment
  body manipulation, large modules incur quadratic-ish parse cost. The
  design intent of one parse + magic-string surgical edits is broken.
- **Impact:** Bench failure risk against the documented 1.15x and 1.5x SWC
  ratio limits in `tests/benchmark/optimizer-benchmark.test.ts:37,38`.
- **Mitigation:** Track AST nodes through the pipeline by start/end positions
  and reuse the original program. Where reparse is genuinely needed (e.g.,
  to validate text after surgical editing), gate it behind an
  `AGAINST_DESIGN_INTENT` comment so it is not casually copy-pasted.
- **Owner suggestion:** perf/design owner.

### TD-6. 176 `: any`/`<any>` type-erasure annotations across `src/`

- **Severity:** MEDIUM (correctness, maintainability)
- **Top offenders:**
  - `src/optimizer/rewrite/const-propagation.ts` (~20 sites; module starts
    with the explicit comment `// Walker parameters use 'any' since the
    strict Node union doesn't cover these.` at line 19)
  - `src/optimizer/rewrite/inline-body.ts` (3 explicit `any` parameters at
    `:30,:50,:83`)
  - `src/optimizer/rewrite/index.ts:480` (recursive walker)
  - `src/optimizer/transform/jsx.ts:56,78,90,197` and many more
  - `src/optimizer/transform/jsx-children.ts:70,144,163,197`
  - `src/optimizer/rewrite/raw-props.ts` callbacks
  - `src/testing/ast-compare.ts` ~150+ uses
- **Issue:** `tsconfig.json` has `"strict": true`, but the actual AST
  manipulation code largely escapes type checking. Comments like "the strict
  Node union doesn't cover these" indicate the team chose the easy escape
  hatch over typing the parser output. This makes future oxc-parser /
  `@oxc-project/types` upgrades dangerous (the yuku 0.5.0 investigation showed
  AST shape changes go undetected by TypeScript precisely because of
  `any` casts).
- **Mitigation:** narrow the worst offenders progressively — start with
  `walk()` callbacks where `oxc-walker` already provides parameter types.
  Use `AstNode` (defined at `src/ast-types.ts:71`) instead of `any`.
- **Owner suggestion:** typesafety / refactor owner.

### TD-7. SipHash-1-3 byte-encoding logic duplicated across two files

- **Severity:** MEDIUM (hash stability — the project's #1 invariant)
- **Files:**
  - `src/hashing/siphash.ts:30-50` — `qwikHash()` does
    `SipHash13.hash(ZERO_KEY, input)`, packs 8 little-endian bytes,
    base64-encodes, replaces `+`/`/`/`-`/`_`/padding.
  - `src/optimizer/key-prefix.ts:19-34` — `computeKeyPrefix()` does the
    same `SipHash13.hash(ZERO_KEY, relPath)`, the same byte-packing, but
    keeps the first 2 base64 characters with no character substitution.
- **Issue:** The byte-packing is character-for-character identical between
  the two files (8 bytes, little-endian h/l layout). Any future change to
  the SipHash port (e.g. swapping JS lib for native crypto, fixing an
  endianness bug) would have to be remembered in both places. The risk
  is silent divergence between the symbol-name hash and the JSX key prefix
  hash — they would no longer use the same byte representation of the
  same SipHash output.
- **Mitigation:** extract a shared helper
  `siphash13Bytes(key, message): Uint8Array` (or a private export from
  `src/hashing/siphash.ts`) and call it from both sites. One source of
  truth for SipHash byte serialization.
- **Owner suggestion:** hashing owner.

### TD-8. `experimentalRawTransfer: true` is the default parser option

- **Severity:** LOW-MEDIUM (forward-compat)
- **Files:** `src/ast-types.ts:100-102` (the default options object),
  `src/optimizer/utils/parse.ts:8` (passes those options to every
  `parseSync` call).
- **Issue:** `experimentalRawTransfer` is, per its name, an experimental
  oxc-parser feature. The yuku/oxc-parser 0.5.0 investigation already
  documented behavioural deltas (TS-only-node preservation) — this flag is
  the most likely future source of similar shape changes. There is no
  feature-detection or fallback for environments where this flag is
  removed/renamed in a future oxc-parser release.
- **Mitigation:** wrap parse calls so a single config switch can disable
  the flag, and add a smoke test that verifies a fixed expected AST shape
  (e.g., a tiny TSX file) so any unexpected oxc-parser output change is
  caught at suite startup, not deep in transform code.
- **Owner suggestion:** parser-integration owner.

### TD-9. `loop-hoisting.ts` exports detection helpers but no caller injects `q:p` / `q:ps` into segment bodies

- **Severity:** MEDIUM (parity feature gap, masked by TD-2)
- **Files:**
  - `src/optimizer/loop-hoisting.ts` — exports `detectLoopContext`,
    `buildCaptureProp`, `generateParamPadding`. The module header docstring
    promises "injects q:p/q:ps props for iteration variable access".
  - `src/optimizer/transform/jsx-elements-core.ts:13,77,91` — the only
    `buildCaptureProp` callers; they emit q:p only inside JSX
    *elements* (HTML element scope), not into the segment body that the
    SWC optimizer puts q:p into.
  - `src/optimizer/transform/event-capture-promotion.ts:380,472` — invokes
    `generateParamPadding` to set the (`_, _1`) param prefix on extracted
    handlers but does not inject the corresponding q:p/q:ps prop.
- **Failing tests (4):**
  - `tests/optimizer/transform.test.ts:515` (EVT-01: HTML element event renaming)
  - `tests/optimizer/transform.test.ts:679` (LOOP-01/02/05: for-of loop)
  - `tests/optimizer/transform.test.ts:713` (LOOP-05: for-i loop)
  - `tests/optimizer/transform.test.ts:762` (LOOP-01: parent-level .map())
- **Why convergence.test.ts is silent on this:** see TD-2 — `stripQpProperties`
  removes `q:p` from both sides before comparison.
- **Mitigation:** ~3-5 file change in `src/optimizer/transform/` to wire
  loop-context-aware q:p emission into segment-body codegen. SWC reference:
  `swc-reference-only/transform/...`. This is multi-day work and out of
  scope for any single small PR. Track as its own ticket.
- **Owner suggestion:** optimizer feature owner.

### TD-10. `seg.extension` leading-dot inconsistency

- **Severity:** LOW (test-only)
- **Files:**
  - `src/optimizer/extract.ts:101-113` — `determineExtension` returns
    dot-prefixed (`.tsx`/`.ts`/`.js`).
  - `src/optimizer/transform/segment-generation.ts:378,798` — strip the
    leading dot when writing `SegmentMetadataInternal.extension`.
  - `src/optimizer/transform/segment-generation.ts:759` — passes the
    dot-prefixed `ext.extension` to `postProcessSegmentCode`.
  - `tests/optimizer/transform.test.ts:787` — asserts `seg.extension`
    matches `/\.(tsx|ts|js)$/` (i.e. **with** dot), but
    `match-these-snaps/*.snap` and `convergence.test.ts` rely on the
    no-dot form for parity.
- **Issue:** Three writers in three different conventions; one test asserts
  the form that conflicts with the snapshot corpus.
- **Mitigation:** keep current "no leading dot" convention on
  `SegmentAnalysis.extension` (matches snapshots), relax the test in
  `transform.test.ts:787` to `.toMatch(/\.?(tsx|ts|js)$/)`, document the
  contract in `src/optimizer/types.ts:72` (the
  `SegmentAnalysis.extension` field declaration).
- **Owner suggestion:** test-infra owner.

### TD-11. `classifyConstness` returns `const` for member expressions on imports — diverges from SWC

- **Severity:** LOW-MEDIUM (parity)
- **Files:** `src/optimizer/transform/jsx.ts:228-234`
  ```
  228:    case 'MemberExpression':
  229:    case 'StaticMemberExpression':
  230:    case 'ComputedMemberExpression': {
  231:      const obj = exprNode.object;
  232:      if (obj && obj.type === 'Identifier' && importedNames.has(obj.name)) return 'const';
  233:      return 'var';
  234:    }
  ```
- **Issue:** SWC `is_const.rs` returns `var` for *all* member expressions
  regardless of object identity. Test
  `tests/optimizer/jsx-transform.test.ts:80` documents this: comment "SWC
  is_const.rs treats ALL member expressions as var regardless of import
  status". Two failing tests confirm:
  - `jsx-transform.test.ts:80` (returns var for `styles.foo`)
  - `jsx-transform.test.ts:282` (puts imported member exprs in varProps)
- **Mitigation:** delete the import-aware branch (`if (obj && obj.type === 'Identifier' && importedNames.has(obj.name)) return 'const';`) so all member expressions return `'var'`. One-line change.
- **Owner suggestion:** parity owner.

### TD-12. Stale magic number `209` in 4 test files (corpus is now 210)

- **Severity:** LOW (test-only, but produces 6 of the 57 always-failing
  tests in the regression analysis)
- **Files:**
  - `tests/hashing/naming.test.ts:76,78`
  - `tests/hashing/siphash.test.ts:31,33`
  - `tests/testing/batch-runner.test.ts:33,35,49,157,181`
  - `tests/testing/snapshot-parser.test.ts:144,147,148,151`
  - `tests/optimizer/convergence.test.ts:4,57` (comments + describe label)
  - `tests/optimizer/failure-families.test.ts:29` (describe label)
  - `tests/optimizer/segment-only-debug.test.ts:4,45` (says 210, mostly correct)
- **Issue:** Commit `d5ae4a4 chore: refresh upstream optimizer snapshots
  and references` brought `match-these-snaps/` from 209 to 210 entries; six
  hard-coded `209` assertions break.
- **Mitigation:** prefer
  `readdirSync(SNAP_DIR).filter(f => f.endsWith('.snap')).length` and
  assert `>= 209` so the corpus can grow without test maintenance. Failing
  that, bump to 210.
- **Owner suggestion:** test-infra owner.

---

## Hash-Stability Risks

### HS-1. SipHash JS port has not been validated against Rust output for inputs ≥ 256 bytes

- **Severity:** LOW-MEDIUM (latent; current corpus is short-input only)
- **Files:** `src/hashing/siphash.ts` (uses `siphash@1.1.0` `lib/siphash13.js`),
  `src/optimizer/key-prefix.ts:20`.
- **Issue:** `siphash` package writes `buf[7] = ml;` (where `ml` is the
  message length) — implicitly truncated to a u8 via Uint8Array assignment.
  This matches Rust `SipHasher13::finish()` which also writes the length as
  a u8. For inputs whose UTF-8 length is `>= 256`, this is correct only
  because both implementations modulo 256. The corpus tests at
  `tests/hashing/siphash.test.ts` use only short symbol/path strings
  (`renderHeader1_div_onClick`, etc.) so this byte path is exercised but
  not stressed. There is no test for inputs at or above the 256-byte
  boundary.
- **Mitigation:** add a hash test for an input whose length is exactly 255,
  256, 511, 512 bytes (e.g. a long display name) and compare against a
  known-good SipHash-1-3 reference. The Rust `qwik` repo ships test vectors
  in `is_const.rs` / `transform.rs`; cross-validate.
- **Owner suggestion:** hashing owner.

### HS-2. `siphash.test.ts` skips 7 known edge-case files plus all `loc=[0,0]` and `name==hash` segments

- **Severity:** LOW (acknowledged debt)
- **Files:** `tests/hashing/siphash.test.ts:46-94`. Skipped files:
  ```
  qwik_core__test__example_build_server.snap
  qwik_core__test__example_capture_imports.snap
  qwik_core__test__example_prod_node.snap
  qwik_core__test__example_qwik_react.snap
  qwik_core__test__example_strip_server_code.snap
  qwik_core__test__relative_paths.snap
  qwik_core__test__should_preserve_non_ident_explicit_captures.snap
  ```
- **Issue:** These files exercise hash code paths the test cannot validate
  (server-stripped segments with `loc=[0,0]`, explicit named QRLs where
  `name==hash`, external-module origins with `../` prefix, CSS-import
  segments). The test header comment honestly says "These will be handled
  during optimizer implementation in later phases." Until they are, hash
  divergence in those code paths is undetected.
- **Mitigation:** when the optimizer implements each edge case, add a
  paired hash assertion for the corresponding snapshot.
- **Owner suggestion:** hashing owner.

### HS-3. `qwikHash` is silently order-sensitive; one path uses `getBasename` to derive context portion

- **Severity:** LOW (correctness, currently OK)
- **Files:** `src/hashing/naming.ts:90-104`, `src/hashing/siphash.ts:21-30`.
- **Issue:** The hash input is *raw concatenated bytes* of `(scope ?? '') +
  relPath + displayName` — there are no separators. Any change to either
  side's path normalization that shifts a `/` from the relPath to the
  displayName side (or omits a slash) silently corrupts the hash without
  changing the visible output. Rust's `to_slash_lossy` is the canonical
  source; `pathe` is the chosen JS equivalent.
- **Mitigation:** add a unit test that parameter-slides a single
  trailing slash from `relPath` to `displayName` and asserts the hash
  changes (and equals a fixed expected value). This documents the
  invariant.
- **Owner suggestion:** hashing owner.

---

## Fragile Areas

### FR-1. `event-capture-promotion.ts` carries an explicit oxc-walker workaround

- **Severity:** MEDIUM
- **File:** `src/optimizer/transform/event-capture-promotion.ts:209-288`
- **Issue:** Block comment from line 209: "Workaround: oxc-walker's
  `getUndeclaredIdentifiersInFunction` does not report for-statement init
  variables (e.g., `i` in `for(let i=0;...)`) or for-in left variables
  (e.g., `key` in `for(const key in obj)`) as undeclared, even though
  they're not declared within the handler function itself. For-of variables
  ARE reported." The workaround scans the body text with regex
  (`getWholeWordPattern(iterVar).test(bodyText)`) for each enclosing loop's
  iterVars and additionally walks the AST to find intermediate function
  scopes for `let`/`var` declarations referenced in `while`/`do-while`
  loop bodies.
- **Risk:** brittle in three ways: (a) a fix in upstream oxc-walker would
  silently double-report; (b) regex word-boundary matching catches comments
  and string literals; (c) the inner `walk(program, ...)` invocation walks
  the entire program for *each* extraction-loop pair, an O(extractions × loops × programSize) pattern.
- **Mitigation:** file an upstream issue against oxc-walker; until fixed,
  add a regression test to `tests/optimizer/event-handler-transform.test.ts`
  that locks in the workaround's expected output for each loop type.
- **Owner suggestion:** event-capture owner.

### FR-2. `tryWrapJsxTextArrows` mutates source text via line-based regex heuristics

- **Severity:** MEDIUM
- **File:** `src/optimizer/input-repair.ts:79-163`
- **Issue:** Repairs JSX text containing `>` by rewriting offending lines
  to `{"text"}`. Heuristics are line-based:
  - `BRACKET_ONLY_LINE = /^[)\]};,]+$/` (line 103)
  - skip-rules at line 118-126 detect "skippable" lines using `startsWith`
    on `<`, `{`, `//`, `/*`, `*`
  - companion-line scan at line 145-156 keeps consuming bracket-only
    lines until the next `<`-starting line
- **Risk:** the heuristic is plausibly correct for SWC-emitted snapshots
  but undefined on user code that interleaves comments, multi-line JSX
  expressions, or unusual whitespace. Any miscoding silently corrupts
  source before parsing. There is no oxc-parser variant tested against
  this repaired form to confirm the repair didn't introduce new errors —
  the only fallback is `result.program.body.length > 0` (which a malformed
  but parseable input would satisfy).
- **Mitigation:** add fuzz tests with edge-case JSX text; assert that
  successful repair preserves the count of `JSXElement` nodes and that
  the repair never emits invalid escape sequences.
- **Owner suggestion:** parser-recovery owner.

### FR-3. `extract.ts` regex string match for `@jsxImportSource` directive

- **Severity:** LOW
- **File:** `src/optimizer/extract.ts:216`
  ```
  const hasNonQwikJsxImportSource = /\/\*\s*@jsxImportSource\s+(?!@qwik|@builder\.io\/qwik)\S+/.test(source);
  ```
- **Issue:** suppresses JSX $-attr extraction when a non-Qwik
  `@jsxImportSource` is set. Done via raw text regex over the entire
  source, not via comment AST nodes. Risks:
  - matches inside string literals or template literals
  - misses `// @jsxImportSource` (line comment form)
  - allowlist is hardcoded (`@qwik`, `@builder.io/qwik`); any new Qwik
    package scope (e.g. `@qwik.dev/router`) is treated as non-Qwik
- **Mitigation:** parse the leading comments via `oxc-parser`'s comment
  array (`module.comments`) and inspect them as structured data.
- **Owner suggestion:** extract owner.

### FR-4. Hardcoded developer-specific paths in benchmark + profile tests

- **Severity:** LOW (test-only, but blocks any non-original developer)
- **Files:**
  - `tests/benchmark/optimizer-benchmark.test.ts:33,35`
    ```
    const QWIK_PACKAGES_DIR = '/Users/jackshelton/dev/open-source/qwik/packages';
    const WORST_CASE_FILE = '/Users/jackshelton/dev/open-source/qwik/packages/qwik/src/core/tests/component.spec.tsx';
    ```
  - `tests/benchmark/profile-deep.test.ts:6,7` — same `/Users/jackshelton/...`
    constants.
- **Issue:** `optimizer-benchmark.test.ts` is gated on
  `QWIK_SWC_BINDING` env var (good — see commit 45e6a55), but the
  hardcoded path is reachable for any developer who happens to set that
  env. `profile-deep.test.ts` is *not* env-gated and will fail
  unconditionally for anyone who is not Jack Shelton.
- **Mitigation:** read both paths from env (`QWIK_PACKAGES_DIR`,
  `WORST_CASE_FILE`) with `describe.skip()` fallback if unset. Apply the
  same env-gate pattern from `optimizer-benchmark.test.ts:42-44`.
- **Owner suggestion:** benchmark owner.

### FR-5. `snapshot-options.ts` uses `/user/qwik/src/` as fixture filesystem root

- **Severity:** LOW (test-only)
- **Files:** `tests/optimizer/snapshot-options.ts:13,53,72,88`,
  `tests/optimizer/inline-strategy.test.ts:39,47`,
  `tests/optimizer/dev-mode.test.ts:20,28,62,66`.
- **Issue:** the path `/user/qwik/src/` is a synthetic fixture root used
  to match Rust SWC test fixtures. It is *not* a real path. This is
  intentional — but there is no comment explaining it, and contributors
  reading the snapshot tests assume it's a typo or environment leak. The
  path also won't work on Windows.
- **Mitigation:** add a comment at the top of `snapshot-options.ts`
  explaining the convention; consider switching to a Windows-friendly
  fixture form (`C:/user/qwik/src/` or normalize via `pathe`).
- **Owner suggestion:** test-infra owner.

---

## Performance Bottlenecks

### PERF-1. Repeated whole-program walks for cross-cutting checks

- **Severity:** MEDIUM
- **Files:**
  - `src/optimizer/transform/event-capture-promotion.ts:246-284` — walks
    the whole program, *per extraction*, *per enclosing loop*, *per
    while/do-while*, looking for declarations.
  - `src/optimizer/extract.ts:218-636` — single big walk, but builds
    `parentMap` (line 213) by inspecting every node — this is fine, but
    the walk re-inspects each JSX attribute branch multiple times via
    `parentMap.get(...)` chains.
  - `src/optimizer/variable-migration.ts` (449 lines), `signal-analysis.ts`
    (960 lines) — each walk and re-walk for separate analyses.
- **Issue:** the bench gate at
  `tests/benchmark/optimizer-benchmark.test.ts:37` is 1.15x SWC. Every
  added cross-cutting walk eats into that budget. Currently we do not
  share walk passes between extract / capture-analysis / migration /
  signal-analysis — each pass re-traverses the AST.
- **Mitigation:** introduce a single visitor pipeline pass (oxc-walker
  supports multiple `enter`/`leave` handlers per traversal). Prioritize
  consolidating the three module-cleanup walks at
  `module-cleanup.ts:69,121,189`.
- **Owner suggestion:** perf owner.

### PERF-2. `ast-compare.ts` runs ~30 normalization passes per comparison

- **Severity:** LOW (test-time only)
- **File:** `src/testing/ast-compare.ts:92-174` (`normalizeProgram`).
- **Issue:** each test comparison runs roughly 30 sequential passes,
  including some that walk the whole AST multiple times (`walkAndReplace`
  is called by `normalizeVoidZero`, `normalizeBooleanLiterals`,
  `walkAndReplace` itself defined at `:3183`). For a 209-snapshot corpus
  this multiplies into noticeable test run time.
- **Mitigation:** combine independent passes into a single visitor, or
  short-circuit when no transforms apply.
- **Owner suggestion:** test-infra owner.

---

## Test Coverage Gaps

### TC-1. No test for `repairInput` `tryRemoveUnmatchedParens`'s "excess !== 1" fallthrough

- **Severity:** LOW
- **File:** `src/optimizer/input-repair.ts:60`
- **Issue:** the function returns `null` for any case where excess
  closing parens > 1, so two-paren mismatches are silently un-repaired.
  No test in `tests/optimizer/` exercises this branch.
- **Mitigation:** add a unit test asserting `repairInput()` returns
  the original source unchanged for two-excess input.

### TC-2. No assertion on `map: null` invariant when `sourceMaps: true`

- **Severity:** MEDIUM (related to TD-4)
- **Files:** none — this is the missing test.
- **Issue:** `TransformModulesOptions.sourceMaps` is accepted but ignored.
  No test verifies this is even consistent. A user passing
  `sourceMaps: true` gets `map: null` with no error or warning.
- **Mitigation:** either add a runtime check that `throw`s when
  `sourceMaps: true`, or add a test asserting the current behavior so the
  contract is documented.

### TC-3. JSX fixture covering loop hoisting + event renaming has no green path

- **Severity:** MEDIUM (related to TD-9)
- **Files:** `tests/optimizer/transform.test.ts:515,679,713,762`,
  `tests/optimizer/loop-hoisting.test.ts` (212 lines, all asserting on
  helpers, not end-to-end).
- **Issue:** there is no end-to-end test of `transformModule()` that
  produces a segment containing `q:p` in its output. The convergence
  comparator (TD-2) strips q:p before comparison, so even matching
  fixtures wouldn't surface a regression.
- **Mitigation:** add `tests/optimizer/loop-hoisting-e2e.test.ts` that
  runs `transformModule` on a fixture with a known SWC output and
  asserts the literal segment-code string contains `"q:p"`.

### TC-4. Hash test does not cover `scope` parameter

- **Severity:** LOW
- **Files:** `src/hashing/siphash.ts:21-30` (signature accepts
  `scope: string | undefined`), `tests/hashing/siphash.test.ts` (does
  not pass a non-undefined scope anywhere).
- **Issue:** the `scope` argument has no positive test. SWC's
  `transform.rs` uses scope for code-split bundles; if a scope-using
  consumer ever lands, we have no hash-stability evidence.
- **Mitigation:** add a hash test with a non-empty scope and a known
  expected output (compute via Rust SipHash13 with the same input).

---

## Security Considerations

### SEC-1. `parseSync(filename, ...)` accepts user-controlled filenames in input

- **Severity:** LOW
- **Files:** `src/optimizer/utils/parse.ts:8`,
  `src/optimizer/transform/index.ts:99` (`relPath` derived from
  `input.path`).
- **Issue:** `oxc-parser` uses the filename to drive parser mode (TSX vs
  TS vs JS) and to format error messages. A malicious filename like
  `'; rm -rf /'` cannot reach a shell here, but it does flow into
  diagnostic strings (`Diagnostic.file` field at
  `src/optimizer/types.ts:124`) which downstream consumers might
  display in HTML. There is no path normalization or input sanitization.
- **Mitigation:** none required for the optimizer itself, but downstream
  consumers should HTML-escape `Diagnostic.file`. Document this in
  `types.ts`.

### SEC-2. `tests/benchmark/optimizer-benchmark.test.ts:50` does `require(QWIK_SWC_BINDING)`

- **Severity:** LOW (test-only, requires malicious env)
- **File:** `tests/benchmark/optimizer-benchmark.test.ts:50`
- **Issue:** `QWIK_SWC_BINDING` is read directly into `require()` — a
  developer-set env var loads arbitrary native code. This is intended
  (the env is meant to point at the Qwik NAPI binding) but worth
  documenting because anyone running CI with this env set is granting
  the env-setter code-execution.
- **Mitigation:** add a comment at line 50; consider validating the
  path resolves under a known-safe prefix.

---

## Architectural Constraints

### AR-1. Single-pass, single-threaded — appropriate for current scope

- **Severity:** N/A
- **Files:** `src/optimizer/transform/index.ts:90` (sync `transformModule`).
- **Note:** `transformModule` is fully synchronous and processes inputs
  sequentially. No worker threads, no parallelism. This matches the SWC
  binding's API contract and is the right choice. Documented here as a
  constraint, not a concern.

### AR-2. No global mutable state

- **Severity:** N/A (good)
- **Note:** module-level constants only (`ZERO_KEY` in
  `src/hashing/siphash.ts:11`, `RAW_TRANSFER_PARSER_OPTIONS` in
  `src/ast-types.ts:100`, regex literals). No singletons, no
  side-effect imports. Good.

### AR-3. `wholeWordPatternCache` is a process-global memo

- **Severity:** LOW (intentional)
- **File:** `src/optimizer/transform/post-process.ts:120`.
- **Issue:** `wholeWordPatternCache` grows unboundedly with the set of
  identifier names seen across all `transformModule` calls. In a long-running
  Vite dev server, this is essentially unbounded.
- **Mitigation:** wrap with a `Map`-with-LRU or clear the cache between
  builds. For now, low-priority because identifier names are small and
  bounded by the size of all source code seen.

---

## Anti-Patterns

### AP-1. "Convergence test as dashboard" pattern erodes parity guarantees

The combination of TD-2 (aggressive normalization) + TD-3 (test-as-measurement)
+ TD-1 (auto-regenerated tracked snapshots) means the convergence suite can
report "passing" even when the optimizer's output is silently drifting from
SWC. This is the dominant correctness risk of the project.

**Fix together:** revert TD-1 (don't track `ts-output/`), audit TD-2's
normalizer for behavior-masking transforms (especially `stripQpProperties`,
`stripDotSBodies`, `stripCapturesDeclarations`), and either gate or relax
TD-3 so a true regression cannot hide in the long-tail of pre-existing
failures.

### AP-2. "Reparse the wrapped body" idiom is duplicated 8+ times

Pattern: synthesize a wrapped source like `const __X__ = (${bodyText})`,
parse it, walk the wrapper's first declaration's `init`. Sites: see TD-5.
This is a workaround for the fact that the original parse result was
discarded. Each instance independently chooses a synthetic filename, a
wrapper prefix, and an offset bookkeeping convention.

**Fix:** the helper at `src/optimizer/utils/transform-session.ts` already
exists (`createTransformSession`, `createFunctionTransformSession`). Migrate
the remaining sites in `rewrite/const-propagation.ts`,
`rewrite/inline-body.ts`, and `segment-codegen.ts:289` to use it.

### AP-3. Two-source-of-truth: comment says one thing, code does another

Examples:
- `src/optimizer/extract.ts:583` comment says "components: on* -> eventHandler,
  rest -> jSXProp" — the code does that, but SWC actually treats *all* $-attrs
  on components as `jSXProp`. The comment is the bug.
- `src/optimizer/loop-hoisting.ts:5` docstring says "injects q:p/q:ps props
  for iteration variable access" — no code does this end-to-end (TD-9).
- `tests/optimizer/convergence.test.ts:7` says "this is a measurement tool
  -- not all tests are expected to pass yet" — the assertions still fail
  the suite.

**Fix:** comment-vs-reality drift like this should be detected by a
periodic doc-sweep.

---

*Concerns audit: 2026-05-05*

# Convergence Failure Triage

**Date:** 2026-05-05
**Total tests:** 212 | **Passing:** 178 | **Failing:** 34

## Summary

Of the 34 failing convergence tests, 18 are parent-module mismatches and 16 are
segment-metadata/body mismatches. The failures cluster around six structural gaps
in the TypeScript optimizer relative to the Rust SWC reference: (1) `_rawProps`
rewriting for destructured/aliased component props, (2) variable migration that
should hoist parent-only-used module decls into the segment instead of leaving them
in the parent, (3) const-folding / const-propagation across the `$`/segment boundary
(literal arithmetic, `cond ? a : b` reduction, propagating literal `const` bindings
into the segment that consumes them), (4) "stripped"-segment placeholder emission
when `stripCtxName` / `stripEventHandlers` apply (TS just skips the extraction
instead of emitting an `export const X = null;` placeholder), (5) external/module
path normalisation (`../node_modules/@qwik.dev/...`, `components/component.tsx`)
being collapsed to just the basename, and (6) name-collision rewriting between user
identifiers and the optimizer's injected `qrl` / `componentQrl` imports. A residual
seventh cluster groups one-off bugs with a shared shape: per-segment metadata
fields the convergence harness inspects (`parent`, `ctxName`) drift on tests where
both the AST and the segment lookup match.

## Clusters

### Cluster 1: `_rawProps` rewrite of destructured / aliased component props

**Failing tests in this cluster:** 5

**Common root cause:** When a `component$` / `componentQrl` callee is given an
arrow with a destructuring pattern (`({ data }) => ...`, `({count, some=1+2,
hello=CONST, stuff: hey, ...rest} ) => ...`), or when the body declares an alias
of `props` (`const { data } = props`), the SWC reference rewrites every member
access to flow through a single `_rawProps` parameter and rewrites the function's
formal parameter list to `(_rawProps)`. The TS optimizer leaves the original
destructuring in place, so member accesses in the segment body resolve against
local destructured names instead of `_rawProps.<key>`.

**Where it lives in the code:** `src/optimizer/rewrite/raw-props.ts`,
`src/optimizer/utils/props-field-rewrite.ts`,
`src/optimizer/segment-codegen/body-transforms.ts`,
`src/optimizer/rewrite/const-propagation.ts` (alias resolution).

**Representative example:**
- Test: `destructure_args_inline_cmp_block_stmt`
- Expected (`match-these-snaps/qwik_core__test__destructure_args_inline_cmp_block_stmt.snap`):
  ```
  export default ((_rawProps)=>{
      return _jsxSorted("div", { ..., "q-e:click": q_..., "q:p": _rawProps }, ...);
  });
  // segment body:
  export const test_div_q_e_click_pFqTss400MA = (_, _1, _rawProps)=>{
      _rawProps.data.selectedOutputDetail = 'options';
  };
  ```
- Actual (`ts-output/qwik_core__test__destructure_args_inline_cmp_block_stmt.snap`):
  ```
  export default ({ data }) => {
      return _jsxSorted("div", ..., { "q-e:click": () => {
          data.selectedOutputDetail = "options";
      } }, null, 3, "u6_0");
  };
  // segment body:
  export const test_div_q_e_click_pFqTss400MA = (_, _1, data) => {
      data.selectedOutputDetail = 'options';
  };
  ```
- Delta: TS keeps the inner destructured name `data` in both the parent JSX prop
  and in the segment, instead of promoting `_rawProps` and rewriting `data` →
  `_rawProps.data` everywhere it crosses the boundary. Note the parent-side delta
  is even worse: the `q-e:click` handler is left inline (an anonymous arrow) instead
  of being extracted to a QRL with a `q:p: _rawProps` slot.

**All tests in this cluster:**
- `destructure_args_inline_cmp_block_stmt`
- `destructure_args_inline_cmp_block_stmt2` — alias variant: `({...props}) => { const {data} = props; ... }`
- `destructure_args_inline_cmp_expr_stmt` — expression-bodied arrow form
- `example_props_optimization` — destructured `count`, `some=1+2`, alias `stuff: hey`, rest spread, defaulted; combines all four sub-features
- `should_wrap_prop_from_destructured_array` — destructured array prop binding does not flow through `_rawProps[i]`

**Suggested phase scope:** Extend the props-rewrite logic in
`src/optimizer/rewrite/raw-props.ts` and the matching segment-side transform in
`src/optimizer/segment-codegen/body-transforms.ts` to handle (a) nested
destructuring, (b) renaming aliases (`stuff: hey`), (c) defaulted bindings
(`some = 1+2`), (d) rest spread (`...rest`), and (e) the `const { x } = props`
late-alias pattern that converts to `_rawProps.x`. The Rust source of truth lives
in `swc-reference-only/props_destructuring.rs`. This cluster also requires the
parent-side JSX writer to extract the `on*$` handler into a real QRL slot rather
than leaving the inline arrow.

---

### Cluster 2: Stripped-segment placeholder emission (`stripCtxName` / `stripEventHandlers`)

**Failing tests in this cluster:** 3

**Common root cause:** When transform options include `stripCtxName` and/or
`stripEventHandlers`, the SWC optimizer still emits a segment file for each
matching extraction, but the body is replaced with `export const <symbolName> =
null;` and `loc: [0, 0]`. The TS optimizer skips emitting the segment entirely (or
emits the un-stripped body), producing a different segment count and missing the
opaque "this was stripped" marker.

**Where it lives in the code:** `src/optimizer/strip-ctx.ts`,
`src/optimizer/transform/segment-generation.ts:392/812` (the two emission sites),
`src/optimizer/transform/index.ts` (Phase 5 dispatch), and the option-handling for
`stripCtxName` / `stripEventHandlers` in `src/optimizer/types.ts`.

**Representative example:**
- Test: `example_strip_server_code`
- Expected:
  ```
  ============================= test.tsx_serverless_handler_ddV1irobfWI.js (ENTRY POINT)==
  export const s_ddV1irobfWI = null;
  ```
  with metadata `loc: [0, 0]`, `captures: false`, `ctxName: 'serverLoader$'`.
- Actual (`ts-output/...`): segment is missing entirely; instead a different
  segment named `Parent_component_serverLoader_k1L0DiPQV1I.js` appears (TS named
  the extraction by call-context instead of by the identifier passed to
  `serverLoader$(handler)`).
- Delta: TS does not honor the strip options, and additionally derives the
  segment name from the enclosing component context rather than from the function
  identifier supplied as the marker call's argument.

**All tests in this cluster:**
- `example_strip_server_code` — `stripCtxName: ['server']`; also exposes the
  named-callback display-name bug above.
- `example_strip_client_code` — `stripCtxName: ['useClientMount$']` plus
  `stripEventHandlers: true`; TS emits the un-stripped segments instead of `null`
  placeholders, and additionally collapses the path from
  `components/component.tsx` to `component.tsx` (see Cluster 5).
- `example_noop_dev_mode` — same shape: `mode: 'dev'`, `stripCtxName: ['server']`,
  `stripEventHandlers: true`. Expected has 5 placeholder segments, TS emits 0.

**Suggested phase scope:** Implement strip emission in
`src/optimizer/transform/segment-generation.ts` so a stripped extraction still
produces a `TransformModule` with body `export const ${symbolName} = null;`,
`loc: [0, 0]`, and the original ctxKind/ctxName preserved for metadata. Also
correct the marker-call display-name derivation so `serverLoader$(handler)` names
the segment after the `handler` identifier, matching `swc-reference-only/transform.rs`.

---

### Cluster 3: Const propagation / const folding across the segment boundary

**Failing tests in this cluster:** 7

**Common root cause:** The Rust optimizer aggressively (a) folds compile-time
expressions during JSX prop emission, e.g. `("true" + 1 ? "true" : "")` →
`'true'`; (b) propagates parent-scope `const` literal bindings into the
referencing segment so the parent declaration can be deleted; and (c) flattens
nested member-access destructuring chains rather than leaving them as
intermediate locals. The TS optimizer keeps the unfolded expression forms and the
intermediate locals in place. The downstream effect is that *parent-only
declarations get re-exported by mistake* (`export { I2 as _auto_I2 }`), or
*segment bodies grow extra `const` lines* the harness won't normalise away.

**Where it lives in the code:** `src/optimizer/rewrite/const-propagation.ts`,
`src/optimizer/const-replacement.ts`, the JSX prop classification in
`src/optimizer/transform/jsx.ts:228-234` (folding/`is_const` parity, see CONCERNS
TD-11), `src/optimizer/variable-migration.ts` (decision of what stays in parent
vs moves to segment vs gets `_auto_` re-exported).

**Representative example:**
- Test: `example_invalid_segment_expr1`
- Expected parent body (`match-these-snaps/qwik_core__test__example_invalid_segment_expr1.snap:64`):
  ```
  export const App_component_ckEPmXZlub0 = ()=>{
      useStylesQrl(q_App_component_useStyles_t35nSa5UV7U);
      return q_App_component_1_w0t0o3QMovU;
  };
  ```
  with the `const style = ...` and `const render = ...` *folded into their
  respective segments* — `App_component_useStyles_t35nSa5UV7U` becomes
  `` `${css1}${css2}` `` and `App_component_1_w0t0o3QMovU` becomes the JSX-returning
  arrow.
- Actual:
  ```
  export const App_component_ckEPmXZlub0 = () => {
      const style = `${css1}${css2}`;
      useStylesQrl(q_App_component_useStyles_t35nSa5UV7U);
      const render = () => { return (...) };
      return q_App_component_1_w0t0o3QMovU;
  };
  ```
- Delta: TS leaves both intermediate `const` bindings inside the parent
  component instead of propagating their initializers into the consuming segments
  and deleting them from the parent.

**All tests in this cluster:**
- `example_invalid_segment_expr1` — `const style = ...; useStyles$(style)` and
  `const render = ...; $(render)` should both have their initializers inlined into
  the segment body.
- `example_capture_imports` — same pattern with `useStyles$(style)`/`useStyles$(css)`;
  also produces a phantom extra segment `App_component_useStyles_1_xBK4W0ZKWe8`.
- `example_getter_generation` — JSX prop `prop: ("true" + 1 ? "true" : "")` should
  fold to `prop: 'true'`; TS leaves the conditional as-is.
- `example_use_optimization` — `useStore({...}).value.countNested.hello.bye.italian.ciao`
  chained destructure should be flattened back into a single member-access in JSX.
- `example_invalid_references` — module-level destructure
  `const [I2, ...] = obj` should remain in the parent only because the segment
  references those names; TS additionally adds `export { I2 as _auto_I2 }`...
  re-exports that the snapshot does not have.
- `example_self_referential_component_migration` — analogous: TS exports
  `_auto_ComponentB` even though only `ComponentA` is used after migration.
- `fun_with_scopes` — top-level `inlinedQrl(null, serverFnHash, data.slice(1))`
  default-export: TS emits the wrapper body verbatim, expected drops it down to
  the QRL call only (server-fn hash propagation).

**Suggested phase scope:** Audit `const-propagation.ts` to (i) recognise more
literal-init forms (template-literal-of-imports, simple member chains, `cond ?
a : b` with both branches literal); (ii) inline those initialisers into the
*single* segment that references them; (iii) remove the original `const`
binding from the parent; (iv) tighten `variable-migration.ts` so a
parent-only-referenced module-level decl does not get an `_auto_` re-export.
Additionally land the one-line `is_const` parity fix called out as CONCERNS
TD-11 (`src/optimizer/transform/jsx.ts:228-234`) and add a literal-conditional
folder that handles the `prop: 'true'` case.

---

### Cluster 4: Captures-vs-paramNames classification + lightweight functional components

**Failing tests in this cluster:** 5

**Common root cause:** The Rust optimizer distinguishes between (a) genuine
`component$`-extracted segments where outer-scope identifiers are delivered via
`_captures = useLexicalScope()` and exposed as a same-line
`const x = _captures[0], y = _captures[1];` prologue, and (b) loop-hoisted
event-handler segments that get the `(_, _1, slotParam)` parameter pattern. The
TS optimizer is mixing these two: handlers nested inside a *non-component$*
function (a "lightweight" function declaration) are getting the event-handler
parameter style instead of the captures prologue, and conversely some genuine
event-handler-in-loop segments are getting captures injected without the slot
param. This shows up additionally as bad parent-side wiring, e.g. an event
handler attached to a *component element* (`<Cmp on...$={fn}>`) being classified
as `'eventHandler'` rather than `'jSXProp'` (the `Concerns A1` regression).

**Where it lives in the code:** `src/optimizer/extract.ts:489-495`/`:583-590`
(ctxKind classification — see CONCERNS A1), `src/optimizer/capture-analysis.ts`,
`src/optimizer/transform/event-capture-promotion.ts:209-288` (the FR-1 oxc-walker
workaround), `src/optimizer/segment-codegen/body-transforms.ts` (captures
prologue emission).

**Representative example:**
- Test: `example_lightweight_functional`
- Expected segment (`match-these-snaps/qwik_core__test__example_lightweight_functional.snap`,
  `Button_button_q_e_click_6YaNiKLqRnQ.tsx`):
  ```
  import { _captures } from "@qwik.dev/core";
  export const Button_button_q_e_click_6YaNiKLqRnQ = ()=>{
      const color = _captures[0], text = _captures[1];
      return console.log(text, color);
  };
  ```
- Actual:
  ```
  export const Button_button_q_e_click_6YaNiKLqRnQ = (_, _1, color, text) =>
      console.log(text, color);
  ```
- Delta: TS uses the loop-hoisted slot-parameter style `(_, _1, color, text)`
  for what is actually a non-loop, non-component$ event handler. The Rust
  reference uses the `_captures` prologue style because the enclosing function
  `Button` is a plain function declaration (lightweight functional component),
  not a `component$`.

**All tests in this cluster:**
- `example_lightweight_functional`
- `example_functional_component_capture_props` — destructure-heavy props produce
  a segment whose body should be a `_captures` prologue; TS instead emits an
  inline capture array that references `v1`/`v2`/`v3` literals.
- `example_immutable_analysis` — `<Component on...$={fn}>` mis-classified as
  `'eventHandler'` (the open A1 regression in `CONCERNS.md`); also see test
  body — minor whitespace-only differences in the captures prologue.
- `example_qwik_react` — async `useTaskQrl` body needs the captures prologue
  unpack `[hostElement, props, reactCmpQrl, store] = useLexicalScope()`; TS
  loses the helper rewrite entirely.
- `example_component_with_event_listeners_inside_loop` — segment bodies look
  right but per-segment metadata (parent, captures order) does not match.

**Suggested phase scope:** Tighten the captures-vs-paramNames decision in
`src/optimizer/capture-analysis.ts` and the segment-body emitter in
`src/optimizer/segment-codegen/body-transforms.ts` to key off "is the closest
enclosing extraction a `component$`/`componentQrl`?" rather than on whether a
loop is present. Land the A1 regression fix in `src/optimizer/extract.ts:585-589`
in the same change. Re-validate the three pre-existing loop-hoisting tests
flagged in CONCERNS TD-9.

---

### Cluster 5: External / sub-directory module path normalisation

**Failing tests in this cluster:** 4

**Common root cause:** When `options.filename` is `../node_modules/...` or
`components/...` (anything other than the bare basename), the Rust optimizer
preserves the full relative path in the parent module header
(`============================= ../node_modules/.../index.qwik.mjs ==`) and in
each segment filename / `path` metadata field. The TS optimizer collapses to the
basename: parent header becomes `index.qwik.mjs`, and `path: ''` instead of
`path: '../node_modules/@qwik.dev/router'`. This compounds into segment-name
divergence (`index.qwik.mjs_GetForm_...` vs
`../node_modules/@qwik.dev/router/index.qwik.mjs_GetForm_...`) and missing
segments altogether for some inputs. The same path-prefix bug also affects
extension propagation: expected `.mjs`, actual `.js`.

**Where it lives in the code:** `src/optimizer/path-utils.ts` (`computeRelPath`,
`computeOutputExtension`), `src/optimizer/extract.ts` (segment filename
construction — `displayName`, `canonicalFilename`), `src/optimizer/transform/index.ts`
(path threading from the `options.filename` into segment metadata).

**Representative example:**
- Test: `example_qwik_react`
- Snapshot options:
  `filename: '../node_modules/@qwik.dev/react/index.qwik.mjs'`
- Expected:
  ```
  ============================= ../node_modules/@qwik.dev/react/index.qwik.mjs_qwikifyQrl_component_useWatch_x04JC5xeP1U.mjs (ENTRY POINT)==
  ```
  segment metadata: `path: '../node_modules/@qwik.dev/react'`, `extension: 'mjs'`.
- Actual:
  ```
  ============================= index.qwik.mjs_qwikifyQrl_component_useWatch_x04JC5xeP1U.mjs (ENTRY POINT)==
  ```
  segment metadata: `path: ''`.
- Delta: leading `../node_modules/@qwik.dev/react` directory portion is dropped.

**All tests in this cluster:**
- `example_qwik_react`
- `example_qwik_react_inline`
- `example_qwik_router_client` — same shape; additionally TS is missing 1+
  segments, and produces extension `.js` where SWC produces `.mjs` (extension
  is derived from input filename in SWC).
- `example_strip_client_code` — `filename: 'components/component.tsx'` collapses
  to `component.tsx` (drops the `components/` directory).

Note: `root_level_self_referential_qrl_inline` has a related but distinct
expression of this bug (`./node_modules/qwik-tree/index.qwik.jsx` becomes
`/user/qwik/src/./node_modules/...` with a redundant `./`); it is captured under
Cluster 7 because the failure mode includes other deltas.

**Suggested phase scope:** Audit `src/optimizer/path-utils.ts` and the call site
that sets `path` on `SegmentAnalysis` (`src/optimizer/transform/segment-generation.ts`)
to preserve the full directory portion of the input filename relative to
`srcDir`/`rootDir`. The Rust source for the rule is `to_slash_lossy()` over the
relative path, in `swc-reference-only/transform.rs`. Also propagate the input
extension into the output (so `.qwik.mjs` → segments emit `.mjs`).

---

### Cluster 6: Identifier-collision rewriting for injected imports

**Failing tests in this cluster:** 1 (with two adjacent symptoms in other
clusters)

**Common root cause:** When the user already imports or declares a binding whose
name collides with one of the optimizer's required injected imports
(`qrl`, `componentQrl`, `inlinedQrl`, `_jsxSorted`, `_captures`), the SWC
optimizer renames the user's binding to a numeric suffix (`qrl as qrl1`,
`componentQrl1`) and rewrites all of its references. The TS optimizer does no
collision detection: it injects `import { qrl } from "@qwik.dev/core"` even when
the user already imported `qrl` from `@qwik.dev/core/what`, and leaves user
declarations like `const componentQrl = ...` shadowing our import.

**Where it lives in the code:** `src/optimizer/rewrite-imports.ts`,
`src/optimizer/marker-detection.ts`, `src/optimizer/rewrite/output-assembly.ts`
(injects QRL imports without checking for shadowing).

**Representative example:**
- Test: `example_qwik_conflict`
- Expected (`match-these-snaps/qwik_core__test__example_qwik_conflict.snap`):
  ```
  import { componentQrl } from "@qwik.dev/core";
  import { qrl } from "@qwik.dev/core";
  import { qrl as qrl1 } from '@qwik.dev/core/what';
  ...
  const componentQrl1 = ()=>console.log('not this', qrl1());
  componentQrl1();
  ```
- Actual:
  ```
  import { qrl } from "@qwik.dev/core/what";    // user's original line
  ...
  const componentQrl = () => console.log("not this", qrl());
  componentQrl();
  ```
  No collision rename; injected `qrl` import is missing because the user's `qrl`
  shadows it; the local `componentQrl` is used as the JSX wrapper instead of our
  import.

**All tests in this cluster:**
- `example_qwik_conflict`

**Suggested phase scope:** Add a "scan parent module for identifier collisions
with injected imports" pass before `rewrite/output-assembly.ts` builds the QRL
declarations and the auto-imports. When a collision is detected, rename the
user's binding (and all references) to `<name>1`, and import our binding under
the canonical name. The Rust source is `swc-reference-only/rename_imports.rs` and
the relevant collector lives in `swc-reference-only/collector.rs`.

---

### Cluster 7: Per-test residual deltas (small grab-bag)

**Failing tests in this cluster:** 9

**Common root cause:** Each of these has a small idiosyncratic gap that does not
fit any of the six clusters above. Most concern segment-metadata field drift
(`parent`, `ctxName`, `loc`) on tests where the AST already matches, or
single-feature regressions (`@jsxImportSource react` directive handling,
`prod`-mode symbol renaming for already-inlined `inlinedQrl`s, `mode: 'lib'`
inlining strategy, dev-mode QRL emission with `lo`/`hi` source positions).

**Where it lives in the code:** mostly `src/optimizer/transform/segment-generation.ts`
(metadata construction), with one-off entries in `src/optimizer/extract.ts:216`
(jsxImportSource regex), `src/optimizer/inline-strategy.ts` (lib mode + parsing
existing `inlinedQrl` calls), and `src/optimizer/dev-mode.ts` (devPath/lo/hi).

**All tests in this cluster:**
- `issue_7216_add_test` — segment AST matches; failure is on metadata field
  (likely `parent` is missing/wrong on `test_component_p_q_e_hi_ttOKZbY46GA`, etc.).
- `should_split_spread_props_with_additional_prop5` — TS uses `_jsx` from
  `react/jsx-runtime` instead of `_jsxSplit` for `<div {...props}>`; spread-only
  props should route through `_jsxSplit(...)` with `_getVarProps`/`_getConstProps`.
- `should_mark_props_as_var_props_for_inner_cmp` — TS optimizer is *missing the
  segment file* for `ModelImg_component_iJe6ICWVnyA`; segment count off by one.
- `should_transform_three_nested_loops_handler_captures_outer_only` — segment
  body matches but metadata fields differ (likely `captures`/`captureNames`
  ordering).
- `example_jsx_import_source` — `/* @jsxImportSource react */` directive at
  top of file should switch all JSX in *this file* to `_jsx` from
  `react/jsx-runtime`; TS still emits qwik's `_jsxSorted`. (`extract.ts:216`
  has a regex that detects the directive but only suppresses extraction; the
  emit path doesn't pivot to react/jsx-runtime.)
- `example_lib_mode` — `mode: 'lib'` should emit `inlinedQrl(...)` calls inside
  the parent module (no separate segment files); TS unconditionally splits.
- `example_parsed_inlined_qrls` — inputs already have `inlinedQrl(body, "Name_hash")`
  calls; in `mode: 'prod'` SWC re-symbolises every QRL to `s_<hash>`; TS leaves
  the original `Name_hash` symbols and additionally produces malformed
  `q_STYLES_odz7dfdfdM.s(STYLES);` declarations for QRLs whose body is a bare
  identifier reference.
- `example_optimization_issue_3542` — combination of segment-name and `q-e:click`
  attribute wrapping (`q_X.w([_rawProps])` not emitted); also overlaps Cluster 1
  but the dominant symptom here is the missing `.w([_rawProps])` wrapping.
- `example_reg_ctx_name_segments_hoisted` — `regCtxName: ['server']` plus
  `entryStrategy: hoist`; TS emits `q_X.s(value)` initialisers wrong (treats
  `STYLES` as a captured value rather than as a registered hoisted body).
- `component_level_self_referential_qrl` — TS is missing the segment files
  entirely for `Foo_component_HTDRsvUbLiE`'s nested `useAsync$` extractions, or
  has the wrong `parent` metadata.
- `root_level_self_referential_qrl_inline` — `./node_modules/qwik-tree/index.qwik.jsx`
  filename: TS emits `node_modules/qwik-tree/...` (drops the `./`), prefixes the
  source map with `/user/qwik/src/./node_modules/...` (redundant `./`), and the
  JSX key prefix differs (`zi_0` vs `S+_0` — see `key-prefix.ts`).

**Suggested phase scope:** This bucket is a sweep of small fixes rather than a
single phase. Most of the metadata-field drift can be fixed in
`src/optimizer/transform/segment-generation.ts` by cross-checking each
`SegmentAnalysis` field against the snapshot. The `@jsxImportSource react` case
needs a real path through `jsx.ts` to switch the JSX runtime per-directive.
`example_lib_mode` and `example_parsed_inlined_qrls` would likely best be folded
into a separate "entry strategies" cluster if any new failures land that share
their shape. The two self-referential tests should be re-evaluated after
Cluster 5 (path normalisation) lands, since the path bug masks several of their
sub-symptoms.

## Cross-cluster notes / unclassifiable tests

- The convergence harness's AST normaliser (`src/testing/ast-compare.ts`) strips
  `q:p` / `q:ps` properties from both sides before comparison
  (`stripQpProperties` at `:1230-1247`). Cluster 4 may be partially masked by
  this — the loop-hoisting failures from CONCERNS TD-9 do not show up here, but
  any partial fix to `event-capture-promotion.ts` should be paired with
  end-to-end tests that look for `q:p` literally.
- Several tests fail on multiple clusters at once (e.g. `example_props_optimization`
  is primarily Cluster 1 but its parent module also exhibits Cluster 3
  const-folding gaps; `example_strip_server_code` is primarily Cluster 2 but the
  `serverless_handler` naming bug is its own thing). When a cluster is closed,
  re-run convergence — counts in adjacent clusters will move.
- All 18 parent-failures emit `============================= test.tsx ==`
  whereas the snapshot uses `test.js` / `test.jsx`. The harness's `compareAst`
  retries with a `.tsx` filename, so this is purely cosmetic — it does **not**
  cause failures on its own. Same applies to `/* @__PURE__ */` vs
  `/*#__PURE__*/` formatting and `=>` vs `=>` arrow-spacing differences. The
  normaliser handles all of these.

## Suggested phase order

1. **Cluster 5 (path normalisation)** — first. It is small, mechanically
   localised to `src/optimizer/path-utils.ts` and the segment-metadata
   construction site, and unblocks several Cluster 7 entries
   (`root_level_self_referential_qrl_inline`, `example_qwik_react`,
   `example_qwik_router_client` may move to passing or to a smaller delta).
2. **Cluster 2 (stripped-segment placeholders)** — second. It is also a small,
   well-defined feature gap (one new emission path in `segment-generation.ts`),
   and it is currently producing missing/extra segment counts that destabilise
   diffing for downstream clusters. Land this together with the
   `serverLoader$(handler)` named-callback display-name bug because they share
   a code path.
3. **Cluster 6 (identifier collisions)** — third. One test, one new pre-pass
   over the parent module, an obvious correctness improvement. Low blast radius.
4. **Cluster 4 (captures vs paramNames + A1 regression)** — fourth. The A1 fix
   is a 3-line change called out in CONCERNS, and the captures-vs-paramNames
   reclassification is the single largest unblocker of Cluster 1's segment-side
   work because it shares helpers in `body-transforms.ts`.
5. **Cluster 1 (`_rawProps` rewriting)** — fifth. This is the largest body of
   work and benefits from Cluster 4 having clarified the captures emission path.
   Land in sub-phases by destructuring shape (flat → nested → aliased →
   defaulted → rest-spread).
6. **Cluster 3 (const propagation / folding)** — sixth. Lowest priority because
   most tests in it are masked by intermediate `const` bindings the
   normaliser strips, and because the `is_const` parity fix from CONCERNS TD-11
   is the only narrow-scope item; the rest is open-ended.
7. **Cluster 7 (residual)** — seventh, sweep at the end. Several entries will
   close as a side-effect of clusters 1-6; the remainder are one-off feature
   gaps that should each get their own small phase if they survive.

---

*Triage produced from `pnpm vitest convergence` run on 2026-05-05; 34 failing /
178 passing / 212 total. Source diffs read from `match-these-snaps/` vs
`ts-output/`.*

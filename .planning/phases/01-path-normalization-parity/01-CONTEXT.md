# Phase 1: Path normalization parity - Context

**Gathered:** 2026-05-05
**Status:** Ready for planning

<domain>
## Phase Boundary

The TS optimizer normalizes input filenames consistently with the Rust SWC reference. For an input like `../node_modules/@qwik.dev/react/index.qwik.mjs` or `components/component.tsx`, the full relative directory portion flows through to:

1. The parent module header (e.g. `============================= ../node_modules/@qwik.dev/react/index.qwik.mjs ==`)
2. Each segment filename
3. `SegmentAnalysis.path` (the directory portion — `'../node_modules/@qwik.dev/react'`, not `''`)
4. `SegmentAnalysis.extension` (the original input extension — `'mjs'`, not `'js'`)

Closes the 4 currently-failing path-shaped convergence tests: `example_qwik_react`, `example_qwik_react_inline`, `example_qwik_router_client`, `example_strip_client_code`. Localized to `src/optimizer/path-utils.ts`, `src/optimizer/extract.ts` (segment filename construction), `src/optimizer/transform/index.ts`, and `src/optimizer/transform/segment-generation.ts`.

Out of scope for this phase: anything beyond CONV-01's 4 tests except the opportunistic Cluster 7 closure noted in D-02 below.

</domain>

<decisions>
## Implementation Decisions

### Path layer abstraction
- **D-01:** Mirror Rust's `PathData` shape. Add a `parsePath(src: string, baseDir: string): PathData` to `src/optimizer/path-utils.ts` that returns a typed object matching Rust's `parse_path` field-for-field (`abs_path`, `rel_path`, `abs_dir`, `rel_dir`, `file_stem`, `extension`, `file_name`). Compute it once in `transformModule` (`src/optimizer/transform/index.ts`) and thread it through `extract.ts` (segment filename construction) and `segment-generation.ts` (segment metadata population). Existing scattered helpers (`computeRelPath`, `getBasename`, `getDirectory`, `computeOutputExtension`) get migrated to read from `PathData` fields rather than recomputing from raw strings. Rationale: keeps the Rust↔TS mapping explicit, gives downstream phases a single stable handle, and makes future Rust path tweaks straightforward to port.

### Spillover policy
- **D-02:** Fix Cluster 7's `./node_modules/qwik-tree/index.qwik.jsx` → `/user/qwik/src/./node_modules/...` redundant-`./` bug opportunistically, since it lives in the same `normalize_path`-equivalent code that this phase is already touching. The Rust source's `normalize_path` (`swc-reference-only/parse.rs:963`) collapses `./` and `..` components; mirroring it in the new TS `parsePath` should close `root_level_self_referential_qrl_inline` as a side effect. Document the closure in PLAN.md under "Side-effect closures from Cluster 7" — Phase 7 will re-baseline the residual count and is allowed to find this already gone. Do NOT pursue any other Cluster 7 fixes here.

### Extension propagation
- **D-03:** Researcher MUST verify Rust's exact behavior for the `transpile_ts` × `transpile_jsx` × input-extension matrix in `swc-reference-only/transform.rs` and `parse.rs` before the planner locks the rule. The current TS `computeOutputExtension(sourceExt, transpileTs, transpileJsx)` may be coincidentally right for some flag combinations and wrong for others. Authoritative answer comes from Rust source + `match-these-snaps/` snapshots (especially the four CONV-01 tests' expected segment filenames). The plan must spell out the exact mapping table — no guessing.

### Testing approach
- **D-04:** Add `tests/optimizer/path-utils.test.ts` covering parity boundary cases:
  - `../node_modules/@qwik.dev/react/index.qwik.mjs` → preserves prefix, `extension = 'mjs'`, `file_stem = 'index.qwik'`
  - `components/component.tsx` → preserves `components/`, `extension = 'tsx'`
  - `./node_modules/qwik-tree/index.qwik.jsx` → redundant `./` collapsed (Cluster 7 closure verification per D-02)
  - `.qwik.mjs` double-extension handling (file_stem retains `.qwik`, extension is `mjs`)
  - No-extension paths
  - `srcDir` variants — set/unset/`.`/`./`
  - `to_slash` cross-platform: ensure forward-slash normalization holds on Windows path separators (pathe handles this; lock with a regression test)

  Convergence (`pnpm vitest convergence`) remains the north-star gate per ROADMAP. Unit tests are belt-and-suspenders so future refactors can't regress parity silently.

### Operating constraints (locked from project setup, restated)
- **D-05:** No new modules outside the four listed in ROADMAP success criterion #4. New code lives in `path-utils.ts`; new types live in `types.ts` if they need to leak into the public API (likely they do not — `PathData` is internal). Any temptation to "while we're here, also fix X" is captured as a deferred idea unless it's the D-02 spillover.
- **D-05a (user-widened 2026-05-06):** `src/optimizer/types.ts` is permitted to be modified for the single-field public-API addition `SegmentAnalysis.path: string`. This was confirmed by the user during plan-phase orchestration when the researcher discovered the field's absence. Rationale: the field exists in Rust's `parse.rs:56` and is asserted by all four CONV-01 target snapshots; CONTEXT.md's D-01 implicitly assumed it existed; CONTEXT.md's D-05 "likely they do not" was a conditional, not a lock.
- **D-06:** No regression in the 178 currently-passing convergence tests is permitted at any commit boundary. The plan must include a "regression-check" task that re-runs `pnpm vitest convergence` after every meaningful diff.
- **D-07:** Hash-input bytes flow through path fields. `qwikHash()` in `src/hashing/siphash.ts` is the only hashing path; do not introduce a second hashing call site. The plan must verify that the new `PathData.rel_path.to_slash_lossy()`-equivalent output (used for hash input per `transform.rs:219, :416`) byte-equals what Rust feeds into its hasher for each test fixture.

### Claude's Discretion
- Internal API shape of `PathData` (interface vs class, immutability) — pick whatever reads cleanly with the existing codebase conventions. Use `interface` per CONVENTIONS.md.
- Migration strategy for the existing scattered helpers — atomic rename vs gradual deprecation. Atomic is fine; the scope is small.
- Naming of `parsePath` — `buildPathData`, `resolvePathData`, etc. all acceptable; pick one, justify briefly in the plan.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Rust reference (ground truth)
- `swc-reference-only/parse.rs:922-961` — `PathData` struct definition + `parse_path()` function. The shape we are mirroring.
- `swc-reference-only/parse.rs:963-982` — `normalize_path()`. The `./` / `..` collapse semantics that close Cluster 7's redundant-`./` bug (D-02).
- `swc-reference-only/transform.rs:175-223` — `path_data` consumption, including hash-input formation via `rel_path.to_slash_lossy()` (D-07).
- `swc-reference-only/transform.rs:341-346, :434, :487, :572` — File-name and base-dir consumption sites for segment filename + parent header construction.
- `swc-reference-only/transform.rs:644-654, :946-954, :1024-1032` — `path` and `extension` field assignment in segment-metadata emission.

### Project planning artifacts
- `.planning/research/CONVERGENCE-TRIAGE.md` §"Cluster 5: External / sub-directory module path normalisation" (lines 292-348) — Root-cause analysis, representative example (`example_qwik_react`), and code pointers. **Note:** the triage labels this as "Cluster 5" internally; ROADMAP and REQUIREMENTS reference it as **CONV-01 / Phase 1**. Same content.
- `.planning/research/CONVERGENCE-TRIAGE.md` lines 338-341 — Cluster 7 cross-reference for `root_level_self_referential_qrl_inline` redundant-`./` bug (D-02 side-effect target).
- `.planning/codebase/ARCHITECTURE.md` — Pipeline structure showing where path data flows.
- `.planning/codebase/CONCERNS.md` — Existing risk catalog; check before/after the change to ensure no concern category gets aggravated.
- `.planning/REQUIREMENTS.md` — CONV-01 definition.
- `.planning/ROADMAP.md` Phase 1 success criteria (the 4 named tests + the localization rule).

### Code under modification
- `src/optimizer/path-utils.ts` — Primary surface. Add `parsePath` + `PathData` here.
- `src/optimizer/extract.ts` — Segment filename construction (`displayName`, `canonicalFilename`).
- `src/optimizer/transform/index.ts` — Path threading from `options.filename` into the pipeline.
- `src/optimizer/transform/segment-generation.ts` — `SegmentAnalysis.path` and `extension` assignment.
- `src/hashing/siphash.ts` — Hash input byte verification reference (D-07).

### Test fixtures (oracle)
- `match-these-snaps/example_qwik_react.snap`
- `match-these-snaps/example_qwik_react_inline.snap`
- `match-these-snaps/example_qwik_router_client.snap`
- `match-these-snaps/example_strip_client_code.snap`
- `match-these-snaps/root_level_self_referential_qrl_inline.snap` — D-02 side-effect verification only; Cluster 7 owns the rest of any deltas.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `pathe` package — already in use throughout `path-utils.ts`. Provides `basename`, `dirname`, `extname`, `normalize`, `normalizeString`, `relative` with cross-platform forward-slash semantics. Use it inside `parsePath`; do not introduce `node:path` calls (CONVENTIONS.md anti-pattern).
- `RAW_TRANSFER_PARSER_OPTIONS` in `src/ast-types.ts:100` — Unrelated, but a reminder that the existing oxc parsing flow is "parse once" and should not be re-entered just for path purposes.
- `qwikHash()` in `src/hashing/siphash.ts` — Only hashing path; consumes path bytes via `rel_path.to_slash_lossy()` equivalent.

### Established Patterns
- **Single thin wrapper for parser/path**: `src/optimizer/utils/parse.ts` is the only `oxc-parser` entry; `path-utils.ts` should remain the only path-string entry. `parsePath` fits this pattern.
- **`Ast*` typing convention**: Doesn't apply here (path is not an AST node), but the `interface` over `type` rule for object shapes (CONVENTIONS.md) does — `PathData` is an `interface`.
- **Frozen tables / well-known constants in SCREAMING_SNAKE_CASE**: doesn't apply directly, but if we expose any path constants (e.g. `PATH_DATA_DEFAULTS`), use that style.
- **No reprinting from AST**: not a path concern, but reinforces the "preserve original strings, mutate surgically" mindset that applies equally to filenames.
- **Phase 0 → 6 fixed phase sequence**: path-data resolution happens at the very top of the pipeline (before Phase 0 repair) — it's input-shaping, not transformation.

### Integration Points
- `transformModule(options: TransformModulesOptions)` in `src/optimizer/transform/index.ts:90` — Compute `PathData` here from `options.filename` + `options.srcDir` (or equivalent `baseDir`). This is the single integration site upstream of all consumers.
- `ExtractionResult` records carry path-derived strings (`displayName`, `canonicalFilename`). After D-01, these read from `PathData` fields rather than rebuilding from raw strings.
- `SegmentAnalysis` (`src/optimizer/types.ts`) carries `path` and `extension`. Public API shape is frozen — these field names and types do not change. Only the values they contain change.
- `qwikHash()` consumes path-derived bytes during symbol-name hashing — verify no byte drift against Rust per D-07.

</code_context>

<specifics>
## Specific Ideas

- **Field naming parity**: Use Rust's exact field names where the TS naming convention allows (`relPath`, `absPath`, `relDir`, `absDir`, `fileName`, `fileStem`, `extension`). Rust uses `snake_case`; TS uses `camelCase`. Do not invent new names like `inputPath` or `directory` — keep the Rust↔TS mapping one-to-one for grep-ability against the reference source.
- **Extension WITHOUT leading dot**: Rust's `parse_path` stores `extension` as `mjs` (no dot). Match that. The Rust consumer at `transform.rs:1144` does `import_path.push_str(&self.options.extension)` — but careful: the call sites in `transform.rs:644, :946, :1024` show `extension: self.options.extension.clone()` flowing into `SegmentAnalysis.extension`, and the snapshots show this is the dotless form. Researcher confirms with snapshot oracle.
- **Authoritative answer for ambiguity**: When in doubt, the snapshot in `match-these-snaps/` is the oracle. Don't argue with the Rust source if the snapshot disagrees — report the disagreement and ask the user.

</specifics>

<deferred>
## Deferred Ideas

- **Other Cluster 7 path-adjacent symptoms** — anything that surfaces during Phase 1 implementation that isn't the D-02 redundant-`./` closure goes into the Cluster 7 backlog for Phase 7 to handle.
- **Refactoring scattered path callers more aggressively** — if migrating to `PathData` reveals deeper cleanup opportunities (e.g., a third path helper in some unrelated file), capture as TD note in `.planning/codebase/CONCERNS.md` for a future tech-debt phase. Do not do it here.
- **Source maps** — explicitly out of scope per PROJECT.md.
- **Performance tuning of path operations** — out of scope; "don't get slower" is the only perf rule.

</deferred>

---

*Phase: 1-path-normalization-parity*
*Context gathered: 2026-05-05*

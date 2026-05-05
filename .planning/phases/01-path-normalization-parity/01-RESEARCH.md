# Phase 1: Path normalization parity - Research

**Researched:** 2026-05-05
**Domain:** Path string normalization + cross-language byte parity (Rust `Path` ↔ `pathe` JS)
**Confidence:** HIGH

## Summary

Phase 1 closes 4 path-shaped convergence failures (`example_qwik_react`, `example_qwik_react_inline`, `example_qwik_router_client`, `example_strip_client_code`) plus opportunistically `root_level_self_referential_qrl_inline` (D-02 spillover). The work is mechanically simple but byte-fragile: every output that mentions a filename must reproduce Rust's exact string. Three deltas dominate:

1. **`SegmentAnalysis.path` field is missing entirely from the TS public-API type** (`src/optimizer/types.ts:65-78`). Rust has it (`parse.rs:56`); the four target snapshots all assert on it. Adding it to the TS interface is therefore on the critical path. CONTEXT.md asserts "Public API shape is frozen — these field names and types do not change," but the snapshots show this field IS the contract. Adding a missing public field that the NAPI Rust optimizer already exposes is consistent with that constraint, not a violation. The plan-checker should still flag it for explicit user confirmation.
2. **The TS `extension` value flow always overwrites the source extension** based on `transpile_ts`/`transpile_jsx` (current `computeOutputExtension`), but Rust's mapping has FIVE branches keyed on `(transpile_ts, transpile_jsx, is_type_script, is_jsx)`. The `(false, false, _, _)` case falls through to `path_data.extension` verbatim — which is what `example_qwik_react` (`mjs`) and `example_qwik_router_client` (`mjs`) require. Current TS hard-codes `.tsx`/`.ts`/`.js`/`.jsx` per `extract.ts:101-113` and `path-utils.ts:100-108`, neither of which preserves `mjs`/`cjs`/exotic extensions.
3. **The TS segment file `path` (TransformModule.path) is `canonicalFilename + extension` only**, missing the `rel_dir` prefix. Rust does `[rel_dir, "/", canonical_filename, ".", extension]` (`parse.rs:457-461`). For root-level fixtures this is no-op (rel_dir empty). For the four target tests it drops the `../node_modules/...` or `components/` prefix.

**Primary recommendation:** Add a `parsePath(src, baseDir): PathData` to `src/optimizer/path-utils.ts` mirroring Rust `parse_path` (`parse.rs:932-961`) field-for-field, compute it once in `transformModule`, thread `PathData` through `extract.ts` / `transform/index.ts` / `transform/segment-generation.ts`, add the missing `path` field to `SegmentAnalysis`, and replace `computeOutputExtension` with the verified 5-row matrix from `parse.rs:225-232`. Hash-input bytes are already correct (`buildSymbolName` uses `relPath` directly); D-07 is a verification step, not a code change.

## Architectural Responsibility Map

This phase is pure code-and-types — it operates wholly inside the TS process. There are no client/server tiers. The map below records which **internal layer** each capability lives in (per `.planning/codebase/ARCHITECTURE.md` § Layers):

| Capability | Primary Layer | Secondary Layer | Rationale |
|------------|---------------|-----------------|-----------|
| Path string parsing (`parsePath`) | Utilities (`src/optimizer/path-utils.ts`) | — | `path-utils.ts` is the established single entry for path-string operations; CLAUDE.md "Path Handling — pathe only" anti-pattern forbids `node:path`. |
| `PathData` type definition | Utilities (`src/optimizer/path-utils.ts`) — exported type | — | `PathData` is internal and not part of the public API contract per D-05. Co-located with the function that produces it. |
| `SegmentAnalysis.path` public field | Public API surface (`src/optimizer/types.ts`) | — | Adding a field that Rust already exposes; consumed by Vite plugin. |
| Threading `PathData` from input to consumers | Public API entry (`src/optimizer/transform/index.ts`) | — | Single integration site at the top of `transformModule`. |
| Segment filename construction | Core extraction (`src/optimizer/extract.ts`) | Utilities (reads from `PathData`) | `extract.ts` builds `canonicalFilename`; reads `fileName`/`fileStem` from `PathData` rather than recomputing. |
| Segment metadata population | Public API surface (`src/optimizer/transform/segment-generation.ts`) | — | Two emission sites at `:371-396` (inline) and `:791-815` (segment). Both populate `path` and `extension` from `PathData`. |
| Extension matrix (input ext → output ext) | Utilities (`src/optimizer/path-utils.ts`) | Public API surface (consumed in `transform/index.ts:402`) | Replacement for `computeOutputExtension`. |
| Hash-input byte verification | Hashing (`src/hashing/siphash.ts` consumer) | Tests | No code change — verify D-07 via test, not by modifying `qwikHash` or `buildSymbolName`. |

Tier note: there is no "wrong-tier" risk in this phase. The danger is *over*-spreading — fixing path issues outside the four files in ROADMAP success criterion #4. CONTEXT.md D-05 locks scope; the planner should reject any task touching files outside the locked set unless the scope-alarm in §Code-State §Reusable-Helper-Migration fires.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**D-01 — Path layer abstraction.** Mirror Rust's `PathData` shape. Add a `parsePath(src: string, baseDir: string): PathData` to `src/optimizer/path-utils.ts` that returns a typed object matching Rust's `parse_path` field-for-field (`abs_path`, `rel_path`, `abs_dir`, `rel_dir`, `file_stem`, `extension`, `file_name`). Compute it once in `transformModule` (`src/optimizer/transform/index.ts`) and thread it through `extract.ts` (segment filename construction) and `segment-generation.ts` (segment metadata population). Existing scattered helpers (`computeRelPath`, `getBasename`, `getDirectory`, `computeOutputExtension`) get migrated to read from `PathData` fields rather than recomputing from raw strings.

**D-02 — Spillover policy.** Fix Cluster 7's `./node_modules/qwik-tree/index.qwik.jsx` → `/user/qwik/src/./node_modules/...` redundant-`./` bug opportunistically, since it lives in the same `normalize_path`-equivalent code that this phase is already touching. The Rust source's `normalize_path` (`swc-reference-only/parse.rs:963`) collapses `./` and `..` components; mirroring it in the new TS `parsePath` should close `root_level_self_referential_qrl_inline` as a side effect. Document the closure in PLAN.md under "Side-effect closures from Cluster 7" — Phase 7 will re-baseline the residual count and is allowed to find this already gone. Do NOT pursue any other Cluster 7 fixes here.

**D-03 — Extension propagation (researcher MUST verify).** Researcher MUST verify Rust's exact behavior for the `transpile_ts` × `transpile_jsx` × input-extension matrix in `swc-reference-only/transform.rs` and `parse.rs` before the planner locks the rule. The current TS `computeOutputExtension(sourceExt, transpileTs, transpileJsx)` may be coincidentally right for some flag combinations and wrong for others. Authoritative answer comes from Rust source + `match-these-snaps/` snapshots (especially the four CONV-01 tests' expected segment filenames). The plan must spell out the exact mapping table — no guessing.

**D-04 — Testing approach.** Add `tests/optimizer/path-utils.test.ts` covering parity boundary cases (full enumeration in §Test Plan below). Convergence (`pnpm vitest convergence`) remains the north-star gate per ROADMAP. Unit tests are belt-and-suspenders so future refactors can't regress parity silently.

**D-05 — Operating constraints.** No new modules outside the four listed in ROADMAP success criterion #4. New code lives in `path-utils.ts`; new types live in `types.ts` if they need to leak into the public API (likely they do not — `PathData` is internal). Any temptation to "while we're here, also fix X" is captured as a deferred idea unless it's the D-02 spillover.

**D-06 — Regression check.** No regression in the 178 currently-passing convergence tests is permitted at any commit boundary. The plan must include a "regression-check" task that re-runs `pnpm vitest convergence` after every meaningful diff.

**D-07 — Hash-input bytes flow through path fields.** `qwikHash()` in `src/hashing/siphash.ts` is the only hashing path; do not introduce a second hashing call site. The plan must verify that the new `PathData.rel_path.to_slash_lossy()`-equivalent output (used for hash input per `transform.rs:219, :416`) byte-equals what Rust feeds into its hasher for each test fixture.

### Claude's Discretion

- Internal API shape of `PathData` (interface vs class, immutability) — pick whatever reads cleanly with the existing codebase conventions. **Recommendation:** Use `interface PathData` per CONVENTIONS.md ("Prefer `interface` for object shapes"). Make all fields readonly. Co-locate the type with `parsePath` in `path-utils.ts`. Do NOT export from `types.ts` (internal only, per D-05).
- Migration strategy for the existing scattered helpers — atomic rename vs gradual deprecation. **Recommendation:** Atomic. Keep `getBasename`/`getDirectory`/`getFileStem`/`getExtension`/`stripExtension`/`normalizePath`/`isRelativePathInsideBase` as low-level pathe wrappers (they have callers in `hashing/naming.ts`, `rewrite/output-assembly.ts`, `context-stack.ts`). Delete `computeRelPath` and `computeOutputExtension` because they get replaced by `parsePath` outputs. Keep `computeParentModulePath` (it's still the right helper for `./{file_name}` import paths inside segments).
- Naming of `parsePath` — `buildPathData`, `resolvePathData`, etc. all acceptable. **Recommendation:** `parsePath` — it shadows the Rust `parse_path` symbol exactly, which CONVENTIONS.md ("Field naming parity ... grep-ability against the reference source") implicitly favours. Justify briefly in the plan: "Mirrors Rust `parse_path` for one-to-one cross-reference."

### Deferred Ideas (OUT OF SCOPE)

- **Other Cluster 7 path-adjacent symptoms** — anything that surfaces during Phase 1 implementation that isn't the D-02 redundant-`./` closure goes into the Cluster 7 backlog for Phase 7 to handle.
- **Refactoring scattered path callers more aggressively** — if migrating to `PathData` reveals deeper cleanup opportunities (e.g., a third path helper in some unrelated file), capture as TD note in `.planning/codebase/CONCERNS.md` for a future tech-debt phase. Do not do it here.
- **Source maps** — explicitly out of scope per PROJECT.md.
- **Performance tuning of path operations** — out of scope; "don't get slower" is the only perf rule.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CONV-01 | TS optimizer normalizes input file paths consistently with the Rust SWC reference for `../node_modules/...` and subdirectory paths, so AST output matches `match-these-snaps/` for the 4 currently-failing path-normalization tests. | All sections below. The four target snapshots are the oracle; §Standard Stack pins `pathe` semantics; §Architecture Patterns shows the `PathData` thread; §Don't Hand-Roll prevents reinventing `normalize`/`parse`; §Code Examples shows the verified Rust snippets that drive the mapping. |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `pathe` | ^2.0.3 (already installed) | Cross-platform forward-slash path operations | Already the project's only path library per CLAUDE.md's "Path Handling — pathe only" anti-pattern. `pathe.parse` returns `{root, dir, base, name, ext}` which maps almost directly onto Rust's `PathData` fields. **[VERIFIED: package.json:18, runtime check via Node REPL]** |

`pathe` API surface needed for `parsePath`:

| API | Use | Rust counterpart |
|-----|-----|------------------|
| `pathe.parse(p)` | Get `{dir, base, name, ext}` in one call | `path.parent()` + `file_name()` + `file_stem()` + `extension()` |
| `pathe.normalize(p)` | Backslash → forward-slash; collapse `..` | `to_slash_lossy()` + half of `normalize_path` |
| `pathe.join(a, b)` | Compose `srcDir + relPath` | `PathBuf::join` |
| `pathe.dirname(p)` | Already used; will likely become redundant after migration to `pathe.parse` | `path.parent()` |
| `pathe.basename(p)` | Already used; redundant after migration | `path.file_name()` |
| `pathe.extname(p)` | Already used; redundant after migration | `path.extension()` (with leading dot — see §Common Pitfalls) |

`pathe.relative` — currently used by `computeRelPath`. After D-01, the responsibility migrates into `parsePath`; `pathe.relative` may still be needed if the input `src` is absolute and we need to compute the relative form against `baseDir` (not exercised by the four target fixtures, but a defensive code path).

**[VERIFIED: pathe@2.0.3 published 2024-12-21 per npm registry; current latest at research time. Behavior verified live via Node REPL — see §Code Examples.]**

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `vitest` | ^4.1.4 (already installed) | D-04 unit tests + D-06 convergence regression gate | Phase 1's only validation harness. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `pathe.normalize` for `./` collapse | Custom string-replace | Rejected. `pathe.normalize` already strips redundant `./` segments (verified: `normalize('./node_modules/qwik-tree/index.qwik.jsx')` → `'node_modules/qwik-tree/index.qwik.jsx'`). For `abs_path`, a `pathe.normalize` over the joined path matches Rust's `normalize_path` for the cases this phase needs. **But** preserving the `./` in `rel_path` (for `_jsxSorted` dev-file fields per snapshot lines 50, 55 of `root_level_self_referential_qrl_inline.snap`) requires NOT calling `pathe.normalize` on the relative side. See §Common Pitfalls. |
| `node:path` | already-deployed `pathe` | Forbidden by CLAUDE.md anti-pattern ("Reading `path.basename` / `path.extname` from Node's `path`"). |
| Custom path-parsing struct | `pathe.parse` output massaged into `PathData` | `pathe.parse` returns the right fields — no need to roll our own segmentation. |

**Installation:**

```bash
# Nothing to install — pathe and vitest already present.
```

**Version verification:** `pathe@^2.0.3` is currently `2.0.3` per `pnpm-lock.yaml`. The npm registry shows `2.0.3` as the latest stable release (published 2024-12-21). No version bump needed for this phase. **[VERIFIED: pnpm-lock.yaml + observed `pathe.parse` runtime behavior]**

## Architecture Patterns

### System Architecture Diagram (path-data flow added)

```text
┌───────────────────────────────────────────────────────────────────────┐
│ transformModule(options) — src/optimizer/transform/index.ts           │
│                                                                       │
│   for each input:                                                     │
│   ┌────────────────────────────────────────────────────────────────┐  │
│   │ STEP 0 (NEW) parsePath(input.path, options.srcDir) → PathData  │  │
│   │   { absPath, relPath, absDir, relDir, fileStem, extension,     │  │
│   │     fileName }                                                 │  │
│   └────────────────────┬───────────────────────────────────────────┘  │
│                        │                                              │
│   pathData ────────────┼─→ extractSegments(...,pathData)              │
│                        │     reads relPath, fileStem, fileName        │
│                        │                                              │
│                        ├─→ rewriteParentModule(...,pathData)          │
│                        │     reads relPath (already)                  │
│                        │                                              │
│                        ├─→ computeOutputExtension(pathData,           │
│                        │      transpileTs, transpileJsx,              │
│                        │      isTypeScript, isJsx) → string           │
│                        │     (NEW 5-row matrix)                       │
│                        │                                              │
│                        ├─→ generateAllSegmentModules(ctx)             │
│                        │     ctx.pathData (new field)                 │
│                        │     → segment.path  = pathData.relDir        │
│                        │     → segment.extension = matrix-output      │
│                        │     → TransformModule.path =                 │
│                        │       (relDir ? relDir+"/" : "") +           │
│                        │       canonicalFilename + "." + extension    │
│                        │                                              │
│                        └─→ TransformModule.path (parent) =            │
│                            relDir + "/" + (didTransform ?             │
│                              fileStem+"."+extension : fileName)       │
└───────────────────────────────────────────────────────────────────────┘
```

The diagram shows the data flow. The change is purely additive: add Step 0, thread `PathData`, replace string-derivation sites with field reads.

### Recommended Project Structure

```
src/optimizer/
├── path-utils.ts           # MODIFIED: add parsePath + PathData
├── extract.ts              # MODIFIED: read fileStem/fileName from PathData
├── transform/
│   ├── index.ts            # MODIFIED: call parsePath once, thread it
│   └── segment-generation.ts  # MODIFIED: write path, fix extension propagation

tests/optimizer/
└── path-utils.test.ts      # MODIFIED: add D-04 boundary cases
```

No new files. Four files modified; one test file extended. All within the locked scope per D-05.

### Pattern 1: Mirror Rust struct field-for-field

**What:** Define `PathData` as a TS interface whose fields map one-to-one onto the Rust `PathData` struct, using camelCase but otherwise preserving the names exactly.

**When to use:** Always for cross-language byte-parity work like Phase 1; never invent novel field names like `inputPath` or `directory`.

**Example:**

```typescript
// Source: swc-reference-only/parse.rs:922-930
//
// pub struct PathData {
//   pub abs_path: PathBuf,    // joined srcDir + relPath, normalized
//   pub rel_path: PathBuf,    // input as given (after to_slash_lossy)
//   pub abs_dir: PathBuf,     // dirname of abs_path
//   pub rel_dir: PathBuf,     // dirname of rel_path
//   pub file_stem: String,    // basename without last extension
//   pub extension: String,    // last extension, NO leading dot
//   pub file_name: String,    // basename WITH extension
// }

export interface PathData {
  readonly absPath: string;    // [VERIFIED: parse.rs:923 + :949]
  readonly relPath: string;    // [VERIFIED: parse.rs:924 + :954]
  readonly absDir: string;     // [VERIFIED: parse.rs:925 + :950]
  readonly relDir: string;     // [VERIFIED: parse.rs:926 + :942]
  readonly fileStem: string;   // [VERIFIED: parse.rs:927 + :936]
  readonly extension: string;  // [VERIFIED: parse.rs:928 + :943]  no leading dot
  readonly fileName: string;   // [VERIFIED: parse.rs:929 + :944]
}
```

### Pattern 2: Single-call, single-thread

**What:** Compute `PathData` exactly once per input file, immediately at the top of the per-input loop in `transformModule`, and thread the same object through every consumer. Never recompute fields downstream.

**When to use:** Always — re-deriving fields from raw strings is the source of every bug this phase fixes.

**Example:**

```typescript
// src/optimizer/transform/index.ts (the pattern, not the literal change)
for (const input of options.input) {
  // STEP 0: compute once
  const pathData = parsePath(input.path, options.srcDir);

  // Phase 0..6 now read from pathData instead of re-deriving:
  const repairResult = repairInput(input.code, pathData.relPath);
  const extractions = extractSegments(repairedCode, pathData, ...);
  // etc.
}
```

### Pattern 3: Snapshot-as-oracle for ambiguity

**What:** When Rust source and `match-these-snaps/*.snap` disagree on a path-related output, the snapshot wins, AND the disagreement is documented in RESEARCH.md / PLAN.md so a human can decide whether to file an upstream issue.

**When to use:** Per D-04 / CONTEXT §Specifics: "When in doubt, the snapshot in `match-these-snaps/` is the oracle. Don't argue with the Rust source if the snapshot disagrees — report the disagreement and ask the user."

**Status of this research:** No disagreements observed between Rust source and the four target snapshots. Every Rust-source rule documented below produced output that matched the snap exactly when traced through the four fixtures. See §Open Questions — there are none related to authoritative-source disagreement.

### Anti-Patterns to Avoid

- **Recomputing path strings inside loops or sub-modules.** Once `PathData` is in hand, downstream code reads fields. Never re-call `pathe.basename(relPath)` etc. inside a sub-module — the entire point of D-01 is "single source of truth."
- **Using `node:path` directly.** Forbidden by CLAUDE.md anti-pattern; would lose Windows-backslash normalization. Verified safe: `pathe.normalize('a\\b\\c.tsx')` → `'a/b/c.tsx'`.
- **Stripping `./` from `relPath`.** The dev-mode `_jsxSorted` calls in `root_level_self_referential_qrl_inline.snap` (lines 50, 55) preserve the leading `./` from the input. `pathe.normalize('./foo')` STRIPS the `./` (verified). So `parsePath` MUST NOT call `pathe.normalize` on the rel-side. Use `pathe.normalize` only on the abs-side, after joining `baseDir + src`.
- **Hard-coding extensions in segment metadata.** Current TS `extract.ts:101-113` (`determineExtension`) and `transform/index.ts:451-466` (post-extraction extension downgrade) carry their own bespoke extension-decision logic. The plan must replace these with reads from a single `computeOutputExtension(pathData, transpile_ts, transpile_jsx, is_type_script, is_jsx)` function whose body is the verbatim 5-row Rust match.
- **Writing to `ts-output/`.** Per CONCERNS TD-1, `ts-output/` is auto-regenerated by the test runner. After running `pnpm vitest convergence`, the working tree will have noisy edits. Always `git checkout -- ts-output/` before committing the actual phase-1 code. The plan must include this cleanup step.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Path component parsing | A custom split/join over `/` | `pathe.parse(p)` | Returns `{root, dir, base, name, ext}` covering `file_stem`, `file_name`, `extension`, `rel_dir` in one call. Verified: `pathe.parse('foo/.qwik.mjs')` → `{dir:'foo', base:'.qwik.mjs', ext:'.mjs', name:'.qwik'}` — handles dotfile + double-extension correctly. |
| `..`/`.` collapse | A custom segment loop | `pathe.normalize(p)` | Verified: `normalize('a/b/../c')` → `'a/c'`; `normalize('/user/qwik/src/./node_modules/foo')` → `'/user/qwik/src/node_modules/foo'`. Mirrors Rust `Path::components()` which auto-filters `Component::CurDir`. |
| Backslash → forward-slash | A `.replace(/\\/g, '/')` | `pathe.normalize(p)` | `pathe` does this internally on every call. Verified: `normalize('a\\b\\c.tsx')` → `'a/b/c.tsx'`. |
| Joining `srcDir + relPath` | A `${srcDir}/${relPath}` template | `pathe.join(srcDir, relPath)` | Handles trailing-slash edge cases (`/user/qwik/src/` and `./node_modules/...` join correctly without double `//`). |
| SipHash-1-3 algorithm | A custom hasher | `qwikHash()` in `src/hashing/siphash.ts` | Forbidden by D-07 + CLAUDE.md "Reading bytes / using `crypto` / a different hash" anti-pattern. Phase 1 does NOT touch hashing code. |
| AST-walk to derive paths | A new walker | Read from `PathData` (computed before AST walk) | Path data is determinable purely from `options.filename` + `options.srcDir`. No AST involvement. |

**Key insight:** every problem in this phase has an off-the-shelf `pathe` solution. The risks are not algorithmic; they are *cross-language byte-parity* risks where two implementations agree on intent but emit different bytes (e.g., `./` preservation, leading-dot on extension, trailing slashes). Hand-rolled string ops compound those risks; `pathe` minimizes them.

## Runtime State Inventory

This is not a rename/refactor/migration phase — it is a feature-completion phase that adds a TS field to mirror an existing Rust field. **Section omitted** per the conditional rule in the research playbook.

The closest related concern is **stale `ts-output/` snapshots** (`tests/optimizer/convergence.test.ts:100` rewrites them on every run). Phase 1 does not introduce any new persisted state, but the planner must include a `git checkout -- ts-output/` step in any task that runs `pnpm vitest convergence` and intends to commit. See CONCERNS TD-1.

## Common Pitfalls

### Pitfall 1: `pathe.normalize` strips redundant `./` from relative inputs

**What goes wrong:** `pathe.normalize('./node_modules/foo')` returns `'node_modules/foo'`, dropping the leading `./`. The `root_level_self_referential_qrl_inline.snap` (lines 50, 55) requires `./node_modules/qwik-tree/index.qwik.jsx` — `./` preserved — in `_jsxSorted` `fileName` fields. These come from `path_data.rel_path.to_slash_lossy()` in Rust, which preserves the input string verbatim.

**Why it happens:** Node's path-normalization semantics differ from Rust's `Path::to_slash_lossy()`. Rust's `Path::components()` filters `Component::CurDir` only when iterating components for `normalize_path`, NOT when stringifying via `to_slash_lossy()`.

**How to avoid:** In `parsePath`, the `relPath` field must equal the input `src` after **only** backslash → forward-slash conversion (call `src.replace(/\\/g, '/')`, not `pathe.normalize(src)`). Reserve `pathe.normalize` exclusively for `absPath` and `absDir`, where Rust's `normalize_path` is what we are mirroring.

**Warning signs:** `_jsxSorted` dev-file fields lose the `./` prefix; `root_level_self_referential_qrl_inline` test fails on the `./` portion of dev-file strings.

### Pitfall 2: `extname` returns `.mjs` (with dot); Rust stores `mjs` (no dot)

**What goes wrong:** `pathe.extname('index.qwik.mjs')` returns `'.mjs'`, but Rust's `path.extension()` returns `OsStr::new("mjs")` (no dot). `SegmentAnalysis.extension` in the snapshot is `"mjs"` (no dot — verified at `qwik_core__test__example_qwik_react.snap:143`).

**Why it happens:** Different conventions; Rust's `OsStr` extension API explicitly excludes the separator, Node's `extname` includes it.

**How to avoid:** In `parsePath`, derive `extension` as `parsed.ext.startsWith('.') ? parsed.ext.slice(1) : parsed.ext`. Document this explicitly in a code comment (`// no leading dot — matches Rust path.extension()`).

**Warning signs:** Snapshot diffs show `"extension": ".mjs"` (TS) vs `"extension": "mjs"` (expected). Easy to miss because some other consumers (e.g. `extract.ts:101-113` `determineExtension`) currently emit the dot-prefixed form, then `segment-generation.ts:378, :798` strip it via `replace(leadingDot, "")`. Two writers, two conventions — see CONCERNS TD-10.

### Pitfall 3: `path.parent()` returns empty PathBuf for bare basenames; `pathe.dirname` returns `'.'`

**What goes wrong:** Rust `Path::new("test.tsx").parent()` returns `Some(PathBuf::from(""))` whose `to_slash_lossy()` is `""`. `pathe.dirname('test.tsx')` returns `'.'`. The `SegmentAnalysis.path` field for root-level fixtures must be `""`, not `"."`.

**Why it happens:** Different root-of-tree representations.

**How to avoid:** In `parsePath`, normalize `parsed.dir === '.' ? '' : parsed.dir`. The existing `getDirectory` helper at `path-utils.ts:29-32` already does this — preserve the convention.

**Warning signs:** Snapshot diffs show `"path": "."` (TS) vs `"path": ""` (expected) for root-level files. Currently masked because `SegmentAnalysis.path` doesn't exist in TS at all — the field is missing, not wrong-valued.

### Pitfall 4: Double-extension `.qwik.mjs` is NOT special-cased by Rust

**What goes wrong:** Tempting to think `index.qwik.mjs` should yield `extension="qwik.mjs"` and `fileStem="index"`. Rust's `path.extension()` returns ONLY the last extension component, and `file_stem()` strips ONLY that component. Verified against `example_qwik_react`: `displayName="index.qwik.mjs_qwikifyQrl_component"` — note `index.qwik.mjs_` is the FULL `file_name`, not the stem. And `extension="mjs"` only.

**Why it happens:** Rust `OsStr::extension()` is documented to return "the extension of `self.file_name`, if possible" — i.e., everything after the LAST dot of the basename, with the basename's leading-dot files (`.qwik.mjs`) treated as "name part = `.qwik`, ext = `mjs`". Verified: `pathe.parse('foo/.qwik.mjs')` → `{name: '.qwik', ext: '.mjs'}` (matches Rust).

**How to avoid:** Trust `pathe.parse`. Don't write a "smart" double-extension splitter.

**Warning signs:** `extension` string ends up as `qwik.mjs`; `fileStem` ends up as `index`. Convergence diff shows segment filename like `index_qwikifyQrl_component_*.mjs` instead of `index.qwik.mjs_qwikifyQrl_component_*.mjs`.

### Pitfall 5: `displayName` prefix is `file_name`, NOT `file_stem`

**What goes wrong:** In Rust `transform.rs:434`: `display_name = format!("{}_{}", &self.options.path_data.file_name, display_name);` — uses `file_name` (with extension). Easy to assume `file_stem` based on intuition (most code uses stems, not full filenames).

**Why it happens:** Rust prefixes display names with the full filename to disambiguate `.tsx` vs `.ts` source files in the same directory.

**How to avoid:** Already correct in current TS code (`extract.ts:208` passes `fileName = getBasename(relPath)`, and `buildDisplayName` uses it). No change needed for displayName, but the planner must verify the migration doesn't accidentally swap to `fileStem`.

**Warning signs:** displayName loses the extension portion (e.g. `index_qwikifyQrl_component` instead of `index.qwik.mjs_qwikifyQrl_component`).

### Pitfall 6: Extension matrix has a `transpile_ts: true, transpile_jsx: false, is_jsx: true` cell that's easy to miss

**What goes wrong:** Naive matrix: "transpileTs → js, transpileJsx → js (or ts if no transpileTs)." Rust adds a `(true, false, _, true)` row that yields `jsx`, NOT `js`: TS-stripped but JSX preserved → output is `.jsx`, not `.js`. Current TS `computeOutputExtension` ignores this combination.

**Why it happens:** `transpile_ts: true, transpile_jsx: false` is rarely tested, and the matrix in TS was simplified.

**How to avoid:** Implement the 5-row matrix verbatim from `parse.rs:225-232`. See §D-03 Extension Propagation Matrix below.

**Warning signs:** A test fixture with `transpileTs: true, transpileJsx: false` and JSX content emits `.js` segments; expected `.jsx`. None of the four CONV-01 target tests trigger this, but `example_transpile_ts_only` (`tests/optimizer/snapshot-options.ts:275-279`) does — must verify it stays passing per D-06.

### Pitfall 7: `did_transform` gates parent-module file renaming

**What goes wrong:** Rust's parent-module path is `path_data.rel_dir.join(a)` where `a = (did_transform && !preserve_filenames) ? "{file_stem}.{extension}" : path_data.file_name`. Easy to think it's always `rel_dir + file_name` regardless of transpilation.

**Why it happens:** SWC renames the parent file when it has stripped TS or transpiled JSX (so `.tsx` becomes `.js`). When neither happens, the original filename is preserved.

**How to avoid:** Mirror the Rust formula exactly. `did_transform` in Rust = `transpile_ts && is_type_script` || `transpile_jsx && is_jsx` (computed at `parse.rs:254, :259`). Encode this explicitly in TS: `const didTransform = (transpileTs && isTypeScript) || (transpileJsx && isJsx)`. Currently TS just emits `relPath` for the parent (which is correct only when `did_transform=false`).

**Warning signs:** `example_strip_client_code`'s parent-module path is `components/component.tsx` (TS) instead of `components/component.js` (expected, snap line 127).

### Pitfall 8: `srcDir` trailing slash handling

**What goes wrong:** `getSnapshotTransformOptions` defaults `srcDir: '/user/qwik/src/'` (trailing slash). Naive joining gives `/user/qwik/src//node_modules/foo` (double slash).

**Why it happens:** Rust uses `Path::join` which handles this; `pathe.join` also handles it. But hand-rolled `${srcDir}/${relPath}` does not.

**How to avoid:** Use `pathe.join(baseDir, relPath)` — verified: `join('/user/qwik/src/', './node_modules/qwik-tree/index.qwik.jsx')` → `'/user/qwik/src/node_modules/qwik-tree/index.qwik.jsx'`. Single slash, no `./`.

**Warning signs:** `absPath` contains `//` or `./` after the srcDir.

## Code Examples

Verified Rust code from `swc-reference-only/`:

### Rust `parse_path` (`parse.rs:932-961`) — the canonical reference

```rust
// Source: swc-reference-only/parse.rs:932-961
pub fn parse_path(src: &str, base_dir: &Path) -> Result<PathData, Error> {
    let path = Path::new(src);
    let lossy = path.to_slash_lossy();
    let path = Path::new(lossy.as_ref());
    let file_stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .map(Into::into)
        .with_context(|| format!("Computing file stem for {}", path.to_string_lossy()))?;

    let rel_dir = path.parent().unwrap().to_path_buf();
    let extension = path.extension().and_then(OsStr::to_str).unwrap();
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .with_context(|| format!("Computing filename for {}", path.to_string_lossy()))?;

    let abs_path = normalize_path(base_dir.join(path));
    let abs_dir = normalize_path(abs_path.parent().unwrap());

    Ok(PathData {
        abs_path,
        rel_path: path.into(),
        abs_dir,
        rel_dir,
        extension: extension.into(),
        file_name: file_name.into(),
        file_stem,
    })
}
```

### Rust `normalize_path` (`parse.rs:963-982`) — closes Cluster 7's redundant-`./`

```rust
// Source: swc-reference-only/parse.rs:963-982
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let ends_with_slash = path.as_ref().to_str().is_some_and(|s| s.ends_with('/'));
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match &component {
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component);
                }
            }
            _ => {
                normalized.push(component);
            }
        }
    }
    if ends_with_slash {
        normalized.push("");
    }
    normalized
}
```

**Critical detail:** `Path::components()` automatically filters `Component::CurDir` (the `.` segments). So this loop never explicitly handles `./` — it's implicitly absorbed. `pathe.normalize` does the same: `normalize('/user/qwik/src/./node_modules/foo')` → `'/user/qwik/src/node_modules/foo'` (verified).

### Rust extension matrix (`parse.rs:225-232`) — D-03 oracle

```rust
// Source: swc-reference-only/parse.rs:225-232
let extension = match (transpile_ts, transpile_jsx, is_type_script, is_jsx) {
    (true, true, _, _) => Atom::from("js"),
    (true, false, _, true) => Atom::from("jsx"),
    (true, false, _, false) => Atom::from("js"),
    (false, true, true, _) => Atom::from("ts"),
    (false, true, false, _) => Atom::from("js"),
    (false, false, _, _) => Atom::from(path_data.extension.clone()),
};
```

Where `is_type_script` and `is_jsx` come from `parse_filename` (`parse.rs:687-700`):

```rust
fn parse_filename(path_data: &PathData) -> (bool, bool) {
    match path_data.extension.as_str() {
        "ts" => (true, false),
        "mts" => (true, false),
        "mtsx" => (true, true),
        "js" => (false, false),
        "mjs" => (false, false),
        "cjs" => (false, false),
        "jsx" => (false, true),
        "mjsx" => (false, true),
        "cjsx" => (false, true),
        _ => (true, true),     // includes "tsx" and any unknown
    }
}
```

### Rust `did_transform` parent-rename gate (`parse.rs:600-605`)

```rust
// Source: swc-reference-only/parse.rs:600-605
let a = if did_transform && !config.preserve_filenames {
    [&path_data.file_stem, ".", &extension].concat()
} else {
    path_data.file_name
};
let path = path_data.rel_dir.join(a).to_slash_lossy().to_string();
```

`did_transform` is set true at `parse.rs:255` (when `transpile_ts && is_type_script`) or `parse.rs:260` (when `transpile_jsx && is_jsx`).

### Rust segment-file path (`parse.rs:451-461`)

```rust
// Source: swc-reference-only/parse.rs:451-461
let path_str = h.data.path.to_string();
let path = if path_str.is_empty() {
    path_str
} else {
    [&path_str, "/"].concat()
};
let segment_path = [
    path,
    [&h.canonical_filename, ".", &h.data.extension].concat(),
]
.concat();
```

### Rust hash input (`transform.rs:218-220, :416-420`) — D-07 oracle

```rust
// Source: swc-reference-only/transform.rs:218-223
let mut hasher = DefaultHasher::new();
let local_file_name = options.path_data.rel_path.to_slash_lossy();
if let Some(scope) = options.scope {
    hasher.write(scope.as_bytes());
}
hasher.write(local_file_name.as_bytes());
```

```rust
// Source: swc-reference-only/transform.rs:412-422
let mut hasher = DefaultHasher::new();
if let Some(hash_override) = hash_override {
    hasher.write(hash_override.as_bytes());
} else {
    let local_file_name = self.options.path_data.rel_path.to_slash_lossy();
    if let Some(scope) = self.options.scope {
        hasher.write(scope.as_bytes());
    }
    hasher.write(local_file_name.as_bytes());
    hasher.write(display_name.as_bytes());
}
```

**TS equivalent already correct:** `src/hashing/naming.ts:90-104`'s `buildSymbolName(displayName, scope, relPath)` calls `qwikHash(scope, relPath, contextPortion)` which feeds `scope + relPath + contextPortion` to SipHash-1-3. The bytes match Rust's `scope + rel_path.to_slash_lossy() + display_name` IF `relPath` byte-equals `rel_path.to_slash_lossy()`. Phase 1's task is to make sure that holds.

### Rust segment metadata population (`transform.rs:643-657, :945-959, :1023-1037`)

```rust
// Source: swc-reference-only/transform.rs:643-657 (representative)
let segment_data = SegmentData {
    extension: self.options.extension.clone(),  // ← from the matrix above
    local_idents,
    scoped_idents,
    captures,
    parent_segment: self.segment_stack.last().cloned(),
    ctx_kind,
    ctx_name,
    origin: self.options.path_data.rel_path.to_slash_lossy().into(),
    path: self.options.path_data.rel_dir.to_slash_lossy().into(),
    display_name,
    need_transform: false,
    hash,
    migrated_root_vars: Vec::new(),
};
```

All three call sites (handle_inlined_qsegment at :643, _create_synthetic_qsegment-Captures at :945, _create_synthetic_qsegment-Function at :1023) follow the same pattern. The `path` field is `rel_dir.to_slash_lossy()` and `extension` is the matrix-decided value (NOT path_data.extension directly — it's the `Atom` carried on `QwikTransformOptions`).

### `pathe.parse` runtime evidence (live REPL at research time)

```javascript
// VERIFIED at research time — pathe@2.0.3
import { parse, normalize, join, dirname, extname, basename } from 'pathe';

parse('../node_modules/@qwik.dev/react/index.qwik.mjs')
// → {root:'', dir:'../node_modules/@qwik.dev/react',
//    base:'index.qwik.mjs', ext:'.mjs', name:'index.qwik'}

parse('./node_modules/qwik-tree/index.qwik.jsx')
// → {root:'', dir:'./node_modules/qwik-tree',     ← ./ PRESERVED in dir
//    base:'index.qwik.jsx', ext:'.jsx', name:'index.qwik'}

parse('components/component.tsx')
// → {root:'', dir:'components',
//    base:'component.tsx', ext:'.tsx', name:'component'}

parse('test.tsx')
// → {root:'', dir:'.',     ← DOT for empty dir, must convert to ''
//    base:'test.tsx', ext:'.tsx', name:'test'}

parse('.qwik.mjs')
// → {root:'', dir:'.', base:'.qwik.mjs', ext:'.mjs', name:'.qwik'}

normalize('./node_modules/qwik-tree/index.qwik.jsx')
// → 'node_modules/qwik-tree/index.qwik.jsx'   ← ./ STRIPPED

normalize('/user/qwik/src/./node_modules/foo')
// → '/user/qwik/src/node_modules/foo'   ← ./ collapsed (matches Rust)

normalize('a/b/../c')
// → 'a/c'   ← .. collapsed (matches Rust)

normalize('a\\b\\c.tsx')
// → 'a/b/c.tsx'   ← backslash → forward slash (Windows safe)

join('/user/qwik/src/', './node_modules/qwik-tree/index.qwik.jsx')
// → '/user/qwik/src/node_modules/qwik-tree/index.qwik.jsx'
```

## D-03 Extension Propagation Matrix (LOCKED)

**Source of truth:** `swc-reference-only/parse.rs:225-232` cross-checked with the four CONV-01 snapshots.

For each cell, the row is `(transpile_ts, transpile_jsx)` and the column is the input extension. Each cell is the segment-file extension AND the `SegmentAnalysis.extension` value (no leading dot).

### Step 1: Input extension → `(is_type_script, is_jsx)` (per `parse.rs:687-700`)

| input ext | is_ts | is_jsx |
|-----------|-------|--------|
| `ts` | true | false |
| `mts` | true | false |
| `mtsx` | true | true |
| `tsx` | true | true (matches `_` fallback) |
| `js` | false | false |
| `mjs` | false | false |
| `cjs` | false | false |
| `jsx` | false | true |
| `mjsx` | false | true |
| `cjsx` | false | true |
| `qwik.mjs` | false | false (the `mjs` branch fires; `qwik` is part of file_stem) |
| `qwik.tsx` | true | true (fallback) |
| (anything else) | true | true (fallback) |

### Step 2: `(transpile_ts, transpile_jsx, is_ts, is_jsx)` → output extension

| transpile_ts | transpile_jsx | is_ts | is_jsx | output extension |
|---|---|---|---|---|
| true | true | _ | _ | `js` |
| true | false | _ | true | `jsx` |
| true | false | _ | false | `js` |
| false | true | true | _ | `ts` |
| false | true | false | _ | `js` |
| false | false | _ | _ | `path_data.extension` (verbatim, e.g. `mjs`, `tsx`, `cjs`) |

### Step 3: Cross-checked against the four target snapshots

| Test | filename | tt | tj | input ext | is_ts | is_jsx | rule | snap extension | match? |
|---|---|---|---|---|---|---|---|---|---|
| `example_qwik_react` | `../node_modules/@qwik.dev/react/index.qwik.mjs` | f | f | `mjs` | f | f | `(false, false, _, _)` → `mjs` | `"mjs"` (line 143) | ✅ |
| `example_qwik_react_inline` | (same) | f | f | `mjs` | f | f | `mjs` | (no JSON segments — inline strategy; parent file path `../node_modules/@qwik.dev/react/index.qwik.mjs` line 1) | ✅ |
| `example_qwik_router_client` | `../node_modules/@qwik.dev/router/index.qwik.mjs` | f | f | `mjs` | f | f | `mjs` | `"mjs"` (lines 1817, 1893, 1922, 1961) | ✅ |
| `example_strip_client_code` | `components/component.tsx` | t | t | `tsx` | t | t | `(true, true, _, _)` → `js` | `"js"` (lines 61, 90, 116) | ✅ |
| `root_level_self_referential_qrl_inline` (D-02 spillover) | `./node_modules/qwik-tree/index.qwik.jsx` | f | t | `jsx` | f | t | `(false, true, false, _)` → `js` | parent header `./node_modules/qwik-tree/index.qwik.js` (line 23) — note `.js` not `.jsx`, matches | ✅ |

### Step 4: Current TS divergence

Current `computeOutputExtension(sourceExt, transpileTs, transpileJsx)` at `path-utils.ts:100-108`:

```typescript
if (transpileTs) return '.js';
if (transpileJsx) return '.ts';
return sourceExt;
```

Divergences from Rust:
1. `(true, true, _, _)`: TS returns `.js`. Rust returns `js`. **Match in semantics, but TS includes leading dot.**
2. `(true, false, _, true)`: TS returns `.js`. Rust returns `jsx`. **DIVERGES.**
3. `(true, false, _, false)`: TS returns `.js`. Rust returns `js`. **Match in semantics.**
4. `(false, true, true, _)`: TS returns `.ts`. Rust returns `ts`. **Match.**
5. `(false, true, false, _)`: TS returns `.ts`. Rust returns `js`. **DIVERGES.**
6. `(false, false, _, _)`: TS returns `sourceExt`. Rust returns `path_data.extension`. **Match in semantics, but TS is dot-prefixed (`sourceExt = .mjs`); Rust no-dot (`mjs`).**

The plan must replace `computeOutputExtension(sourceExt, transpileTs, transpileJsx)` with `computeOutputExtension(pathData, transpileTs, transpileJsx, isTypeScript, isJsx)` returning the no-dot form, and propagate the no-dot convention through every consumer.

`extract.ts:101-113` `determineExtension(argNode, sourceExt)` is a SECOND extension-deciding code path that returns `.tsx`/`.ts`/`.js` based on AST JSX presence. This is NOT part of Rust's matrix and must be removed (its responsibility is subsumed by the post-extraction extension downgrade at `transform/index.ts:451-466`, which itself must be replaced with a single matrix call).

Existing `transform/index.ts:451-466` (the post-extraction "downgrade extensions" loop) must be DELETED. Its logic was the previous workaround for not having the proper matrix; with the matrix in place it becomes redundant and would compete with the new `computeOutputExtension`.

## D-07 Hash-Input Byte-Equivalence Verification

The hash-input bytes already flow correctly through TS — the issue is verifying byte-equivalence with Rust before the plan claims D-07 satisfied.

### Rust hash input
- `scope` (if any) bytes
- `path_data.rel_path.to_slash_lossy()` bytes
- `display_name` bytes (for symbol-name; just `rel_path` for the constructor's hasher at `transform.rs:218-223`)

For `example_qwik_react`:
- `relPath` = `../node_modules/@qwik.dev/react/index.qwik.mjs` (43 bytes UTF-8)
- One segment: `displayName` = `index.qwik.mjs_qwikifyQrl_component_useWatch` → contextPortion = `qwikifyQrl_component_useWatch`
- Hash input bytes: `'' + '../node_modules/@qwik.dev/react/index.qwik.mjs' + 'qwikifyQrl_component_useWatch'`
- Expected hash from snapshot line 140: `x04JC5xeP1U`

### TS hash input (current code, `naming.ts:90-104`)
- `qwikHash(scope, relPath, contextPortion)` writes `scope.bytes + relPath.bytes + contextPortion.bytes` (per `siphash.ts:21-30`)

So the byte sequence is correct **IF AND ONLY IF** `relPath` (as passed into `buildSymbolName`) byte-equals what Rust uses. Currently TS uses `computeRelPath(input.path, options.srcDir)` which does:
1. `pathe.normalize(input.path)` ← **strips `./`!** This is fine for the four CONV-01 tests (none have `./`), but breaks `root_level_self_referential_qrl_inline`'s D-02 spillover where Rust preserves the `./`.

After Phase 1: `pathData.relPath` will be the input `src` after backslash-only conversion (no normalization). For the four targets:
- `example_qwik_react`: `'../node_modules/@qwik.dev/react/index.qwik.mjs'` → unchanged. ✅
- `example_qwik_react_inline`: same. ✅
- `example_qwik_router_client`: same. ✅
- `example_strip_client_code`: `'components/component.tsx'` → unchanged. ✅
- `root_level_self_referential_qrl_inline`: `'./node_modules/qwik-tree/index.qwik.jsx'` → currently TS strips `./` (giving wrong hash); after fix, `./` preserved and hash matches Rust.

### Verification step (must appear in plan)

D-07 verification must produce one assertion per target test that:
1. Computes the exact byte sequence the new `parsePath` feeds into `qwikHash` (concatenated `scope + relPath + contextPortion`).
2. Compares to the snapshot's expected hash via `qwikHash(...) === expectedHash`.

This belongs in `tests/optimizer/path-utils.test.ts` (D-04 boundary cases) as a parameterized test row.

**Pitfalls noted by D-07:**
- **BOM:** TS source code does not BOM-prefix paths. `siphash.ts` has not been observed BOM-leaking — verified by inspection of `siphash.ts:21-30` which uses `TextEncoder().encode(...)` (or whatever the upstream `siphash` package uses). Low risk.
- **Trailing whitespace:** None of the path operations introduce trailing whitespace; `pathe.parse` and the `replace(/\\/g, '/')` step preserve length exactly.
- **Locale:** Path strings here are pure ASCII. UTF-8 encoding determinism holds.

## Existing TS Code State

All current `path-utils.ts` exports and their callers (verified by grep):

| Export | Lines | Callers (file:line) | Action under D-01 |
|--------|-------|---------------------|-------------------|
| `getExtension` | `path-utils.ts:11` | `extract.ts:206`, `transform/index.ts:100` | **Keep** as low-level pathe wrapper. After D-01, callers may switch to `pathData.extension` (no-dot) but the helper itself stays for ad-hoc uses. |
| `stripExtension` | `path-utils.ts:16` | `path-utils.ts:36, :91` (internal) | **Keep**. Used internally by `getFileStem` and `computeParentModulePath`. |
| `getBasename` | `path-utils.ts:24` | `extract.ts:204, :205, :207`, `hashing/naming.ts:96`, `path-utils.ts:87` (internal), `tests/optimizer/path-utils.test.ts:15` | **Keep**. After D-01, `extract.ts` callers move to `pathData.fileName`/`fileStem`. `hashing/naming.ts:96` is a SCOPE-OUT site (see scope alarm below). |
| `getDirectory` | `path-utils.ts:29` | `extract.ts:202`, `path-utils.ts:71` (internal), `tests/optimizer/path-utils.test.ts:16` | **Keep**. `extract.ts:202` migrates to `pathData.relDir`. |
| `getFileStem` | `path-utils.ts:35` | `extract.ts:203`, `context-stack.ts:31` | **Keep**. `extract.ts:203` migrates to `pathData.fileStem`. `context-stack.ts:31` is a SCOPE-OUT site. |
| `normalizePath` | `path-utils.ts:40` | `path-utils.ts` (internal), `tests/optimizer/path-utils.test.ts:14` | **Keep**. Internal use. |
| `computeRelPath` | `path-utils.ts:48` | `transform/index.ts:99`, `tests/optimizer/path-utils.test.ts:3, :22-24` | **DELETE.** Replaced by `parsePath(...).relPath`. The single `transform/index.ts:99` caller migrates. The test cases (lines 22-24) become tests of `parsePath`. |
| `isRelativePathInsideBase` | `path-utils.ts:68` | `rewrite/output-assembly.ts:30, :396`, `tests/optimizer/path-utils.test.ts:7, :28-31` | **Keep**. Used inside the rewrite engine for "import path stays inside srcDir" check. Not path-data adjacent. SCOPE-OUT site. |
| `computeParentModulePath` | `path-utils.ts:83` | `transform/index.ts:48, :415` | **Keep**. Computes `./{file_stem}` or `./{file_name}` for segment imports back to parent. Logic is correct; just consumes `pathData.relPath` instead of separate `relPath` parameter. |
| `computeOutputExtension` | `path-utils.ts:100` | `transform/index.ts:47, :402` | **REPLACE.** New signature `computeOutputExtension(pathData, transpileTs, transpileJsx, isTypeScript, isJsx): string` returning no-dot form. The single caller at `transform/index.ts:402` migrates. |

### Scope alarm

**`hashing/naming.ts:96` and `context-stack.ts:31` and `rewrite/output-assembly.ts:30, :396` use `getBasename`/`getFileStem`/`isRelativePathInsideBase`** — these are OUTSIDE the four files locked in ROADMAP success criterion #4 (`path-utils.ts`, `extract.ts`, `transform/index.ts`, `transform/segment-generation.ts`).

**Status:** Not a violation. These callers consume *the helpers*, not *path data state*. They keep using the helpers as-is. The plan must NOT migrate them to `parsePath`; the helpers remain as low-level shims for these scope-outside callers.

If during implementation someone is tempted to remove `getBasename` (because all in-scope callers migrated), the answer is: don't — `naming.ts:96` still needs it. Likewise for `getFileStem` (`context-stack.ts:31`) and `isRelativePathInsideBase` (`output-assembly.ts:30`).

### Files to modify (locked)

1. `src/optimizer/path-utils.ts` — add `parsePath` + `PathData`; replace `computeOutputExtension`; delete `computeRelPath`.
2. `src/optimizer/extract.ts` — `extractSegments(source, pathData, scope?, ...)` reads `pathData.fileName`/`fileStem`/`relPath`/`extension` instead of recomputing. Delete `determineExtension`.
3. `src/optimizer/transform/index.ts` — `parsePath(input.path, options.srcDir)` at top of input loop; thread `pathData`; replace `computeRelPath` call; replace post-extraction extension downgrade loop (`:451-466`); pass `pathData` and matrix-derived extension into segment context.
4. `src/optimizer/transform/segment-generation.ts` — `segmentAnalysis.path = pathData.relDir`; `segmentAnalysis.extension = pathData-derived no-dot string`; `TransformModule.path = (pathData.relDir ? pathData.relDir + '/' : '') + canonicalFilename + '.' + extension` (segment files); for the parent module emission in `transform/index.ts:514-521`, `path = pathData.relDir + '/' + (didTransform ? fileStem + '.' + extension : fileName)`.
5. `src/optimizer/types.ts` — **add `path: string` field to `SegmentAnalysis`** (between `extension` and `parent`). This is a public-API addition matching Rust. Also update `SegmentMetadataInternal` if needed (it already extends `SegmentAnalysis`, so no separate change).
6. `tests/optimizer/path-utils.test.ts` — extend with D-04 boundary cases.

Five source files + one test file. Item 5 (`types.ts`) is technically OUTSIDE the four files in ROADMAP criterion #4. **Discuss with user:** the snapshot-asserted `path` field is missing from the public TS interface; either we add it (recommended) or convergence cannot pass. CONTEXT.md D-05 implicitly acknowledges this with "new types live in `types.ts` if they need to leak into the public API (likely they do not — `PathData` is internal)" — `PathData` doesn't, but `SegmentAnalysis.path` does. The plan must surface this expansion clearly so the user can confirm the four-file scope rule is relaxed for `types.ts`.

## Test Plan

`tests/optimizer/path-utils.test.ts` already exists with 3 test groups. Phase 1 extends it. Each row's expected output is derived from EITHER a Rust source quote or a snapshot — never from intuition.

### Boundary cases to encode (D-04)

| Group | Input `src` | Input `srcDir` | Expected `relPath` | Expected `relDir` | Expected `fileStem` | Expected `extension` | Expected `fileName` | Expected `absPath` (key check) | Source-of-truth |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `../node_modules/@qwik.dev/react/index.qwik.mjs` | `/user/qwik/src/` | `../node_modules/@qwik.dev/react/index.qwik.mjs` | `../node_modules/@qwik.dev/react` | `index.qwik` | `mjs` | `index.qwik.mjs` | `/user/qwik/node_modules/@qwik.dev/react/index.qwik.mjs` (one `..` consumed `src/`) | `parse.rs:932-961` + `qwik_react.snap:142, :143` |
| 2 | `components/component.tsx` | `/user/qwik/src/` | `components/component.tsx` | `components` | `component` | `tsx` | `component.tsx` | `/user/qwik/src/components/component.tsx` | `parse.rs` + `strip_client_code.snap:60, :61` |
| 3 (D-02) | `./node_modules/qwik-tree/index.qwik.jsx` | `/user/qwik/src/` | `./node_modules/qwik-tree/index.qwik.jsx` | `./node_modules/qwik-tree` | `index.qwik` | `jsx` | `index.qwik.jsx` | `/user/qwik/src/node_modules/qwik-tree/index.qwik.jsx` (no `/./`) | `parse.rs:949` + `self_referential.snap:34` |
| 4 (double-extension) | `foo/.qwik.mjs` | `/user/qwik/src/` | `foo/.qwik.mjs` | `foo` | `.qwik` | `mjs` | `.qwik.mjs` | `/user/qwik/src/foo/.qwik.mjs` | Rust `path.file_stem()` returns `.qwik` for hidden-file double-ext + verified `pathe.parse('foo/.qwik.mjs').name === '.qwik'` |
| 5 (no-extension) | `Makefile` | `/user/qwik/src/` | `Makefile` | `''` (empty, NOT `.`) | `Makefile` | `''` (empty — Rust unwraps `None` differently; see Open Questions) | `Makefile` | `/user/qwik/src/Makefile` | `parse.rs:943` says `unwrap()` — would PANIC on no-extension. So this case won't be exercised by the optimizer normally. Test should assert `parsePath` either throws or returns `''` consistently. Document the choice. |
| 6 (srcDir variants) | `test.tsx` | `''` | `test.tsx` | `''` | `test` | `tsx` | `test.tsx` | `test.tsx` (joined with empty base = same) | `pathe.join('', 'test.tsx')` = `test.tsx` |
| 6b | `test.tsx` | `'.'` | (same) | `''` | `test` | `tsx` | `test.tsx` | `test.tsx` (`.` joined = no-op) | `pathe.join('.', 'test.tsx')` = `test.tsx` |
| 6c | `test.tsx` | `'./'` | (same) | `''` | `test` | `tsx` | `test.tsx` | `test.tsx` | `pathe.join('./', 'test.tsx')` = `test.tsx` |
| 7 (Windows) | `src\\components\\App.tsx` | `C:\\users\\apps` | `src/components/App.tsx` (backslashes converted) | `src/components` | `App` | `tsx` | `App.tsx` | `C:/users/apps/src/components/App.tsx` | `pathe.normalize` handles Windows separators; `support_windows_paths` snapshot in `snapshot-options.ts:146-152` exercises this |
| 8 (D-07 hash byte-equivalence) | `../node_modules/@qwik.dev/react/index.qwik.mjs` | `/user/qwik/src/` | (relPath as Group 1) | — | — | — | — | — | Compute `qwikHash(undefined, relPath, 'qwikifyQrl_component_useWatch')` → assert `=== 'x04JC5xeP1U'` (from `qwik_react.snap:140`) |
| 9 (D-07 hash for D-02) | `./node_modules/qwik-tree/index.qwik.jsx` | `/user/qwik/src/` | (relPath as Group 3) | — | — | — | — | — | Compute `qwikHash(undefined, relPath, 'Tree_component')` → assert `=== 'XMEiO6Rrd3Y'` (from `self_referential.snap:33`) |

### Existing tests (do NOT break per D-06)

The current 3 tests in `path-utils.test.ts:12-33` must continue passing. Of those:
- "normalizes windows-style paths" (`:13`) — independent of `parsePath`, exercises low-level helpers. **Keep as-is.**
- "preserves current `computeRelPath` behavior for paths outside srcDir" (`:21`) — `computeRelPath` is being deleted. **Migrate** these assertions into `parsePath` tests against `relPath`. The cases:
  - `computeRelPath('src/routes/index.tsx', 'src')` returned `'routes/index.tsx'` (stripped srcDir prefix). After D-01: `parsePath('src/routes/index.tsx', 'src').relPath` should return `'src/routes/index.tsx'` (Rust does NOT strip the srcDir from rel_path — it stores the input verbatim). **This is a behavior change.** The test name "preserves current ... behavior" is misleading; the new behavior is Rust-correct, but it differs from the old.
  - The plan must document this change. Cross-check: does any passing convergence test rely on `computeRelPath` stripping? Likely yes — `transform/index.ts:99` uses it everywhere. The new behavior (relPath = input verbatim) might affect `displayName` prefixes for tests where `srcDir` is a sub-path of `input.path`. **Verify by running the full convergence suite after the change** — D-06 mandates this.
- "detects whether a relative import stays within the srcDir-relative tree" (`:27`) — independent of `parsePath`. **Keep as-is.**

### Validation hooks

Per D-04 + D-06, the plan must include a "regression-check" task between every meaningful diff that re-runs `pnpm vitest convergence`. The expected delta is:

- **Baseline:** 178/212 passing (from STATE.md).
- **After Phase 1, target:** 182/212 passing (the 4 CONV-01 tests close).
- **Bonus / D-02 spillover:** possibly 183/212 if `root_level_self_referential_qrl_inline` also closes. PLAN.md must note this is bonus, not required.
- **Hard constraint (D-06):** the 178 baseline-passing tests STAY passing. Any phase-internal commit that drops a previously-passing test must roll back before the next commit.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|---|---|---|---|
| Multiple scattered path helpers, recomputed at each call site | Single `parsePath` returning a struct, computed once per input | Phase 1 (this phase) | Predictable byte-equivalence with Rust. Eliminates the class of "I forgot to slash-normalize" bugs. |
| `computeOutputExtension(sourceExt, transpileTs, transpileJsx)` with 3 branches | `computeOutputExtension(pathData, transpileTs, transpileJsx, isTypeScript, isJsx)` with the 6-row Rust matrix | Phase 1 | Closes the `(true, false, _, true) → jsx` and `(false, false, _, _) → mjs/cjs` cells that were silently wrong. |
| `SegmentAnalysis` missing `path` field | `SegmentAnalysis` has `path: string` matching Rust `parse.rs:56` | Phase 1 | The four target snapshots assert on this field; convergence requires it. |

**Deprecated/outdated:** Nothing in the project as a whole — `path-utils.ts` is being completed, not deprecated. The internal helpers `getBasename`/`getFileStem`/`getDirectory` stay (per scope alarm above).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|---|---|---|
| A1 | Adding `SegmentAnalysis.path: string` is consistent with "API shape is frozen" because Rust already has it. | §Existing TS Code State, item 5 | If user reads "frozen" as "no additions ever," the plan blocks. **Mitigation:** the plan must surface this addition explicitly for user confirmation before implementation begins. |
| A2 | Path-string inputs are always pure ASCII and the SipHash byte-input concatenation is locale-independent. | §D-07 Hash-Input Byte-Equivalence | Low risk; verified by inspecting all four target test fixtures (none contain non-ASCII paths). If a future fixture contains UTF-8 path bytes, the plan's hash-byte assertion still works (UTF-8 is deterministic per spec). |
| A3 | `pathe.parse('foo/.qwik.mjs').name === '.qwik'` matches Rust's `path.file_stem()` for hidden-file double-extension. | §Pitfall 4 | Verified live via Node REPL. No fixture in the four targets exercises this exact form, so it's a defensive case in the unit test. If wrong, only Group-4 unit test fails — convergence unaffected. |
| A4 | `pathe.normalize` and Rust `Path::components()` produce identical output for all paths in the four target fixtures and the D-02 spillover. | §Pattern 1, §Pitfall 1 | Verified via REPL for the specific paths in the fixtures. If `pathe.normalize` differs from Rust on an exotic input (e.g. POSIX-vs-Windows weirdness), only that specific input would diverge. The unit-test matrix exercises Windows separators (Group 7) which is the riskiest cross-platform case. |
| A5 | The `_jsxSorted` dev-file fields in `root_level_self_referential_qrl_inline.snap:50, :55` come from `path_data.rel_path.to_slash_lossy()`, NOT from `dev_path` or some other field. | §D-07, §Pitfall 1 | Inferred from cross-referencing `transform.rs:341-345` (which uses `dev_path` if set, else `rel_path`). The fixture sets `mode: dev` but does NOT set `devPath` (per `snapshot-options.ts:566-572`). So the fallback path runs. **Verify** in plan-phase that this assumption is correct by reading the relevant TS dev-mode emission code; if TS already uses `relPath` here, no change needed. If it uses something else, the planner should flag this as an additional task. |

**If this table is empty:** N/A — five assumptions are documented above. A1 is the highest-impact one and must be confirmed by user before plan execution.

## Open Questions (RESOLVED)

1. **Does adding `SegmentAnalysis.path: string` constitute breaking the "frozen API" rule?**
   - What we know: CONTEXT.md D-05 says "new types live in `types.ts` if they need to leak into the public API (likely they do not — `PathData` is internal)." But `SegmentAnalysis.path` has been present in Rust since the API was designed; it's missing from TS, not "new."
   - What's unclear: whether the user reads "frozen" as no-additions-ever, or no-changes-to-existing-shape.
   - Recommendation: surface this at plan-discuss / plan-checker time. Default action: add the field, document it as completing the existing public-API contract with Rust.
   - **RESOLVED (2026-05-06):** User confirmed during plan-phase orchestration on 2026-05-05; D-05 is widened by one file (`src/optimizer/types.ts`) for this single-field public-API addition. See CONTEXT.md amendment **D-05a (user-widened 2026-05-06)**. Disposed by Plan 02 Task 1.

2. **What is the correct behavior for no-extension paths (test Group 5)?**
   - What we know: Rust `parse.rs:943` calls `.unwrap()` on `path.extension()` — this would PANIC on a no-extension path. SWC test fixtures appear to never include such paths.
   - What's unclear: Should TS `parsePath` panic, or return `''`, or treat the whole basename as the stem with empty extension?
   - Recommendation: throw or return `''` consistently. The plan should pick one (recommend: return `''` to match the spirit of "lossy" path handling — Rust's `unwrap` is a "this never happens in production fixtures" assumption). Document it as defensive code.
   - **RESOLVED (2026-05-06):** `parsePath` returns `extension: ''` defensively when no extension is present. Disposed by Plan 01 Task 1 (the boundary-case row "no-extension paths" in `tests/optimizer/path-utils.test.ts` — Group 5).

3. **Does any currently-passing convergence test rely on `computeRelPath`'s srcDir-stripping behavior?**
   - What we know: `computeRelPath('src/routes/index.tsx', 'src')` returned `'routes/index.tsx'` — stripping the prefix. After D-01, `parsePath` returns the input verbatim (no stripping).
   - What's unclear: how many passing convergence tests pass `srcDir: '/user/qwik/src/'` AND `filename: 'src/something/foo.tsx'`. Searching `snapshot-options.ts` for `'src/'` shows several (e.g. `example_default_export: 'src/routes/_repl/[id]/[[...slug]].tsx'`, `example_default_export_index: 'src/components/mongo/index.tsx'`, `example_default_export_invalid_ident: 'src/components/mongo/404.tsx'`).
   - Recommendation: the plan must include an early "smoke convergence run" task — apply ONLY the `parsePath` + relPath change (not yet adding `path` field or matrix), run convergence, and inspect which previously-passing tests flip. If significant breakage occurs, the plan must add a `srcDir`-stripping step inside `parsePath` (mirroring whatever Rust actually does — TBD by reading the Rust pipeline more carefully). If no breakage, proceed.
   - **Caveat:** Rust `parse_path` doesn't strip `srcDir` from `rel_path` (the input is stored verbatim). The TS `computeRelPath` stripping is a TS invention that may have been compensating for some other downstream gap. The plan should treat its removal as "going from ad-hoc to Rust-correct" but verify empirically.
   - **RESOLVED (2026-05-06):** Disposed via empirical smoke run — Plan 02 Task 2 dispositions any flip in three buckets (no flip → 178 still pass / informational flip with `path:''` placeholder still correct → continue / regression < 178 → revert + combine 02 with 03 atomically). Plan 02 Task 3 enforces the "no commit if convergence < 178" rule (D-06 invariant — see CONTEXT.md D-06).

4. **Does Phase 1 also need to update `transform/index.ts:99-100`'s use of `relPath` and `ext`?**
   - What we know: `transform/index.ts:99-100` currently calls `computeRelPath` and `getExtension` directly into local vars. After D-01, both become `pathData.relPath` and `pathData.extension`.
   - What's unclear: only that this is ONE OF the consumer-update sites; the plan must enumerate all of them. From grep: `transform/index.ts:99, :100, :402, :415, :471` (the `relPath` parameter passed to `rewriteParentModule`).
   - Recommendation: plan-phase enumerates every line that reads `relPath` / `ext` in `transform/index.ts`, audits each, and either (a) replaces with `pathData.X` reads or (b) keeps the local var but recomputes from `pathData`.
   - **RESOLVED (2026-05-06):** Enumerated and verified at Plan 03 Task 1 Step 8 (the per-input loop confirms dev-mode emits the `relPath` field directly via `_jsxSorted` dev-file plumbing). All five sites (`:99`, `:100`, `:402`, `:415`, `:471`) are migrated to read `pathData.relPath` / `pathData.extension`; the `_jsxSorted` dev-file field source uses `pathData.relPath` directly (NOT `dev_path` or some other field) per the per-input loop verification.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Node.js | Test runtime | ✓ | >=20 (per `package.json` engines) | — |
| pnpm | Build/test | ✓ | 10.25.0 (per `packageManager`) | — |
| pathe | Path operations | ✓ | 2.0.3 | — |
| vitest | Test runner | ✓ | 4.1.4 | — |
| oxc-parser | AST parsing | ✓ | 0.124.0 | — |
| TypeScript | Compilation/type-check | ✓ | 5.9.3 | — |

**Missing dependencies with no fallback:** None.

**Missing dependencies with fallback:** None.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | vitest 4.1.4 |
| Config file | `vitest.config.ts` |
| Quick run command | `pnpm vitest run tests/optimizer/path-utils.test.ts` |
| Full suite command | `pnpm vitest run` |
| North-star command | `pnpm vitest convergence` (file-name match against `convergence.test.ts`) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONV-01 | TS optimizer normalizes input file paths consistently with the Rust SWC reference for the four CONV-01 tests | convergence (integration) | `pnpm vitest convergence -t "example_qwik_react"` (and three siblings) | ✅ `tests/optimizer/convergence.test.ts` |
| CONV-01 | `parsePath` produces field values matching Rust `parse_path` for boundary cases (Groups 1-7) | unit | `pnpm vitest run tests/optimizer/path-utils.test.ts` | ✅ (file exists; needs new test rows added) |
| D-07 | Hash bytes equal Rust's `rel_path.to_slash_lossy()` byte-for-byte for the four CONV-01 fixtures | unit (hash-equivalence) | `pnpm vitest run tests/optimizer/path-utils.test.ts -t "hash"` | ✅ (file exists; needs new test rows) |
| D-02 (bonus) | `root_level_self_referential_qrl_inline` also closes after Phase 1 | convergence | `pnpm vitest convergence -t "root_level_self_referential_qrl_inline"` | ✅ |
| D-06 | The 178 baseline-passing tests stay passing | convergence (regression) | `pnpm vitest convergence` (full suite) | ✅ |

### Sampling Rate

- **Per task commit:** `pnpm vitest run tests/optimizer/path-utils.test.ts` (sub-second)
- **Per wave merge:** `pnpm vitest convergence` (~30-60 seconds for the 209-snapshot suite)
- **Phase gate:** Full suite green (`pnpm vitest run`) before `/gsd-verify-work`. Convergence at minimum 182/212.

### Wave 0 Gaps

Existing test infrastructure covers all phase requirements:
- `tests/optimizer/path-utils.test.ts` — exists; needs new test rows from D-04 boundary table.
- `tests/optimizer/convergence.test.ts` — exists; no changes needed; the four CONV-01 tests already run, just currently fail.
- `tests/optimizer/snapshot-options.ts` — already correctly configures all four CONV-01 tests + the D-02 spillover. No changes needed.

No framework install, no config changes, no shared fixtures needed.

## Security Domain

This phase is a pure compile-time path-string operation; no auth, no session, no input from untrusted runtime sources.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A — no authentication surface. |
| V3 Session Management | no | N/A. |
| V4 Access Control | no | N/A. |
| V5 Input Validation | partial | Path strings come from the caller (`TransformModulesOptions.input[].path`). The optimizer is a library invoked by the Vite plugin; the plugin is trusted. CONCERNS SEC-1 documents this: malicious filenames could land in `Diagnostic.file` strings; downstream consumers must HTML-escape. **No new validation required for this phase** — the existing trust boundary is unchanged. |
| V6 Cryptography | no | SipHash use is for QRL identity, not security. The hash inputs are deterministic by-design and cannot be "exploited" — they are part of the public protocol. |

### Known Threat Patterns for path-string normalization

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via `..` in user-controlled `relativePath` | Tampering | Not applicable — the optimizer does not READ from any path; it only stringifies. The `..` in `../node_modules/@qwik.dev/react/index.qwik.mjs` is a legitimate Qwik convention (the Vite plugin emits it for external deps). No filesystem access happens during optimization. |
| Polyglot filenames (filenames that confuse both `basename` and `extension`) | Tampering | Not applicable for the same reason. The matrix in §D-03 deterministically maps the `extension` field; no shell, no filesystem, no eval. |
| Backslash injection on Unix | Tampering | `pathe.normalize` converts to forward-slash unconditionally, eliminating Windows-vs-Unix divergence at the parsing boundary. |

**No additional security controls required for Phase 1.** This is an internal byte-parity pass with no new attack surface.

## Project Constraints (from CLAUDE.md)

The following CLAUDE.md directives constrain Phase 1's implementation. The planner MUST verify each:

1. **NodeNext + ESM:** All new imports in `path-utils.ts`, `extract.ts`, `transform/index.ts`, `transform/segment-generation.ts`, `types.ts` must keep the `.js` extension on relative paths and use bare specifiers for packages (no extension on `pathe`).
2. **Path Handling — pathe only:** Forbidden to import from `node:path`. `parsePath` uses `pathe.parse`, `pathe.normalize`, `pathe.join` exclusively.
3. **Hashing — qwikHash only:** Forbidden to introduce a second hashing call site. Phase 1 verifies hash bytes via test, never via reimplementation.
4. **Single thin parser wrapper:** `path-utils.ts` is the only path-string entry point per Established Patterns. `parsePath` fits this; do not bypass it from any consumer.
5. **`Ast*` typing convention:** N/A — `PathData` is not an AST node.
6. **Interface over type for object shapes:** `PathData` MUST be an `interface`, not a `type`.
7. **No reprinting from AST:** N/A — Phase 1 doesn't touch AST emission.
8. **Re-parsing inside loops:** N/A — Phase 1 doesn't parse.
9. **Walking AST nodes with hand-written `walk()` recursion:** N/A.
10. **No leading dot on `extension`:** New convention from this phase. Document in `types.ts` JSDoc on the `SegmentAnalysis.extension` field.
11. **GSD Workflow Enforcement:** All edits must come through a GSD command (`/gsd-execute-phase 1` after planning).
12. **`./swc-reference-only/` is read-only:** Per `.claude/rules/CONSTRAINTS.md`, never modify reference Rust source. Only read.
13. **`./match-these-snaps/` is read-only:** Same; never modify the snapshot oracle.

## Sources

### Primary (HIGH confidence)

- `swc-reference-only/parse.rs:922-961` — `PathData` struct + `parse_path()` function definitions. Authoritative shape and field-derivation rules.
- `swc-reference-only/parse.rs:963-982` — `normalize_path` for `..`/`./` collapse. Closes Cluster 7 D-02 spillover.
- `swc-reference-only/parse.rs:225-232` — Extension matrix. D-03 oracle.
- `swc-reference-only/parse.rs:600-605` — Parent-module path construction (`did_transform` gate).
- `swc-reference-only/parse.rs:451-461` — Segment-file path construction.
- `swc-reference-only/parse.rs:687-700` — `parse_filename` (input-extension → `is_type_script`/`is_jsx`).
- `swc-reference-only/transform.rs:178-180` — `QwikTransformOptions.path_data`/`extension` fields.
- `swc-reference-only/transform.rs:218-223` — Hash input via `path_data.rel_path.to_slash_lossy()`. D-07 oracle.
- `swc-reference-only/transform.rs:412-422` — Per-extraction hash input. D-07 oracle.
- `swc-reference-only/transform.rs:434` — `display_name = format!("{}_{}", file_name, ...)` — confirms `file_name` (with extension) prefix, not `file_stem`.
- `swc-reference-only/transform.rs:643-657, :945-959, :1023-1037` — Three sites populating `SegmentData.path` and `SegmentData.extension`.
- `swc-reference-only/transform.rs:1141-1145` — Segment import path is `./{canonical_filename}` (no rel_dir).
- `swc-reference-only/transform.rs:5075-5078` — `get_canonical_filename` definition.
- `swc-reference-only/test.rs:7474-7499` — `TestInput::default()` (filename, srcDir, transpile defaults).
- `swc-reference-only/test.rs:3214, :3317, :3328` — Four CONV-01 test configurations.
- `match-these-snaps/qwik_core__test__example_qwik_react.snap` — JSON metadata at lines 134-162, 213-238 (oracle for `path`, `extension`, `displayName`, `canonicalFilename`).
- `match-these-snaps/qwik_core__test__example_qwik_router_client.snap` — JSON metadata at lines 1810-1962 (oracle).
- `match-these-snaps/qwik_core__test__example_strip_client_code.snap` — JSON metadata at lines 53-126 (oracle).
- `match-these-snaps/qwik_core__test__example_qwik_react_inline.snap` — Parent module header at line ~117 (`============================= ../node_modules/@qwik.dev/react/index.qwik.mjs ==`).
- `match-these-snaps/qwik_core__test__root_level_self_referential_qrl_inline.snap` — Lines 23, 34, 50, 55 (D-02 spillover oracle).
- `src/optimizer/path-utils.ts` (full file, 109 lines).
- `src/optimizer/types.ts:65-90` — current `SegmentAnalysis` and `SegmentMetadataInternal`.
- `src/optimizer/extract.ts:34, :101-113, :202-208, :390-413, :505-535, :594-626, :655-688` — current path consumption.
- `src/optimizer/transform/index.ts:47-51, :99-100, :402, :415, :451-466, :471, :513-521` — current path threading.
- `src/optimizer/transform/segment-generation.ts:371-396, :791-815` — current segment metadata emission.
- `src/hashing/naming.ts:90-104` — `buildSymbolName` (consumes `relPath`).
- `tests/optimizer/path-utils.test.ts` (full file, 33 lines).
- `tests/optimizer/snapshot-options.ts:138-162, :566-572, :581-584` — Four CONV-01 + D-02 spillover test option configs.
- `.planning/research/CONVERGENCE-TRIAGE.md:292-348` — Cluster 5 root-cause and code pointers.
- `.planning/research/CONVERGENCE-TRIAGE.md:338-341` — D-02 spillover note.

### Secondary (MEDIUM confidence)

- `pathe@2.0.3` runtime behavior verified live via local Node REPL (`node --input-type=module` invocations during research). Output transcripts captured in §Code Examples.
- `node:os` `Path::components()` semantics for `Component::CurDir` filtering — Rust standard library documented behavior; verified by tracing `normalize_path` on `/user/qwik/src/./node_modules/foo`.

### Tertiary (LOW confidence)

None — all critical claims trace to either Rust source, snapshot, or live Node verification. No WebSearch claims needed.

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — `pathe` is already in use; verified live.
- Architecture: HIGH — `parsePath`/`PathData` shape is a direct mirror of the Rust struct; data flow trace is straightforward.
- Pitfalls: HIGH — every pitfall in §Common Pitfalls was verified against Rust source AND Node REPL behavior, AND traced through at least one of the four target snapshots.
- D-03 extension matrix: HIGH — verbatim from Rust source AND validated against four target snapshots in §D-03.
- D-07 hash byte-equivalence: HIGH — flow analysis is unambiguous; assertion is testable in unit form.
- Open Questions: MEDIUM — three open questions remain that need plan-phase / discuss-phase resolution. None blocks research; all are confined to the implementation strategy.

**Research date:** 2026-05-05

**Valid until:** 2026-06-05 (30 days; this is stable Rust source pinned in `swc-reference-only/`, stable test snapshots pinned in `match-these-snaps/`, and stable `pathe@2.0.3`. Refresh if the SWC reference is updated or `pathe` major version bumps).

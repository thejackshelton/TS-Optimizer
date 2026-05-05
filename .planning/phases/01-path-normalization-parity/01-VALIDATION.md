---
phase: 1
slug: path-normalization-parity
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-05-05
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | vitest 4.1.4 |
| **Config file** | `vitest.config.ts` |
| **Quick run command** | `pnpm vitest run tests/optimizer/path-utils.test.ts` |
| **Full suite command** | `pnpm vitest convergence` |
| **Estimated runtime** | ~5s quick, ~60s full |

---

## Sampling Rate

- **After every task commit:** Run `pnpm vitest run tests/optimizer/path-utils.test.ts` (quick) — must finish < 10s
- **After every plan wave:** Run `pnpm vitest convergence` — must show ≥ 178 passing, no regressions
- **Before `/gsd-verify-work`:** Full convergence suite green AND `pnpm vitest run` (all tests) green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

> One row per task across plans 01-01..01-06. Threat Ref is `—` for every row because Phase 1's `<threat_model>` block on every plan documents "Not applicable — pure compile-time string transformation"; no STRIDE-mitigatable surface exists. Secure Behavior is `N/A` for the same reason.

| Task ID    | Plan  | Wave | Requirement | Threat Ref | Secure Behavior | Test Type                 | Automated Command                                                                                       | File Exists | Status     |
|------------|-------|------|-------------|------------|-----------------|---------------------------|---------------------------------------------------------------------------------------------------------|-------------|------------|
| 01-01-T1   | 01-01 | 0    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "src/optimizer/path-utils.ts" \|\| echo "TypeCheck OK for path-utils.ts"` | ✅          | ⬜ pending |
| 01-01-T2   | 01-01 | 0    | CONV-01     | —          | N/A             | unit                      | `pnpm vitest run tests/optimizer/path-utils.test.ts`                                                    | ❌ W0       | ⬜ pending |
| 01-01-T3   | 01-01 | 0    | CONV-01     | —          | N/A             | integration (convergence) | `pnpm vitest convergence`                                                                               | ✅          | ⬜ pending |
| 01-02-T1   | 01-02 | 1    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "types.ts" \|\| echo "No new tsc errors in types.ts"`                | ✅          | ⬜ pending |
| 01-02-T2   | 01-02 | 1    | CONV-01     | —          | N/A             | integration (convergence) | `pnpm vitest convergence`                                                                               | ✅          | ⬜ pending |
| 01-03-T1   | 01-03 | 1    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "transform/index.ts" \| head -20`                                    | ✅          | ⬜ pending |
| 01-03-T2   | 01-03 | 1    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "(transform/index\|segment-generation)" \| head -10`                 | ✅          | ⬜ pending |
| 01-03-T3   | 01-03 | 1    | CONV-01     | —          | N/A             | integration (convergence) | `pnpm vitest convergence`                                                                               | ✅          | ⬜ pending |
| 01-04-T1   | 01-04 | 2    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "extract.ts" \| head -10`                                            | ✅          | ⬜ pending |
| 01-04-T2   | 01-04 | 2    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "(transform/index\|extract).ts" \| head -20`                         | ✅          | ⬜ pending |
| 01-04-T3   | 01-04 | 2    | CONV-01     | —          | N/A             | integration (convergence) | `pnpm vitest convergence`                                                                               | ✅          | ⬜ pending |
| 01-05-T1   | 01-05 | 3    | CONV-01     | —          | N/A             | unit                      | `pnpm tsc --noEmit 2>&1 \| grep -E "segment-generation.ts" \| head -10`                                 | ✅          | ⬜ pending |
| 01-05-T2   | 01-05 | 3    | CONV-01     | —          | N/A             | integration (convergence) | `pnpm vitest convergence` (must show ≥ 182 passing)                                                     | ✅          | ⬜ pending |
| 01-06-T1   | 01-06 | 4    | CONV-01     | —          | N/A             | integration (convergence) | `pnpm vitest convergence` (final gate, ≥ 182 passing)                                                   | ✅          | ⬜ pending |
| 01-06-T2   | 01-06 | 4    | CONV-01     | —          | N/A             | unit                      | `test -f .planning/phases/01-path-normalization-parity/01-PHASE-SUMMARY.md && grep -E "Phase 1: Path normalization parity" .planning/phases/01-path-normalization-parity/01-PHASE-SUMMARY.md && echo OK` | ✅          | ⬜ pending |
| 01-06-T3   | 01-06 | 4    | CONV-01     | —          | N/A             | unit                      | `grep -E "completed_phases: 1" .planning/STATE.md && grep -E "\[x\] \*\*Phase 1" .planning/ROADMAP.md && grep -E "6/6 \| Complete" .planning/ROADMAP.md && echo OK` | ✅          | ⬜ pending |
| 01-06-T4   | 01-06 | 4    | CONV-01     | —          | N/A             | manual (diff review)      | Human review of `git diff --cached` per checkpoint script                                               | ✅          | ⬜ pending |
| 01-06-T5   | 01-06 | 4    | CONV-01     | —          | N/A             | unit                      | `git log -1 --format=%s \| grep -E "docs\(01-06\)"`                                                     | ✅          | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

*Note: `wave_0_complete: false` reflects that the Wave 0 test file (`tests/optimizer/path-utils.test.ts`) gains its new D-04 / D-07 / D-03 rows at Plan 01 Task 2 — it flips to `true` after that task lands at execute time. The plan is READY (`status: ready`), the work is not yet done.*

---

## Wave 0 Requirements

- [ ] `tests/optimizer/path-utils.test.ts` — boundary cases for `parsePath` (D-04)
  - `../node_modules/@qwik.dev/react/index.qwik.mjs` shape (relPath, relDir, fileStem='index.qwik', extension='mjs', fileName)
  - `components/component.tsx` shape
  - `./node_modules/qwik-tree/index.qwik.jsx` under `srcDir='/user/qwik/src'` — absPath has no `/./` segment (D-02 Cluster 7 closure)
  - `.qwik.mjs` double-extension handling (oracle: snapshot)
  - No-extension paths
  - `srcDir` variants: unset / `''` / `'.'` / `'./'` / absolute
  - Windows-style separator input → forward-slash output (pathe `to_slash` parity)
  - Hash-byte assertion: `qwikHash(undefined, '../node_modules/@qwik.dev/react/index.qwik.mjs', 'qwikifyQrl_component_useWatch') === 'x04JC5xeP1U'` (D-07 / RESEARCH.md finding 4)

*Existing test infrastructure (vitest) covers all other phase requirements — no framework install needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Authoritative-disagreement protocol | CONV-01 | If Rust source and `match-these-snaps/` snapshot disagree on any field, snapshot wins AND the disagreement is logged. Cannot be expressed as a unit assertion — it's a process rule. | If implementer encounters such a case during execution, record it in PLAN.md `## Surprises` section before the disagreement is silently resolved. |
| Plan 06 phase-closure diff review | CONV-01 | Interactive-mode policy (STATE.md `Mode: interactive`) requires user to eyeball the staged STATE.md + ROADMAP.md + PHASE-SUMMARY.md diff before the docs commit lands. | Per Plan 06 Task 4 `<how-to-verify>` script — user types `approved` or describes issues. |

*All other phase behaviors have automated verification via convergence + path-utils unit tests.*

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers `tests/optimizer/path-utils.test.ts` creation
- [x] No watch-mode flags (`--watch` is forbidden in CI)
- [x] Feedback latency < 60s
- [x] `nyquist_compliant: true` set in frontmatter (planner has filled the verification map)
- [x] Anti-regression rule (D-06): convergence suite re-run after every meaningful diff with ≥ 178 passing — enforced in Tasks 01-01-T3, 01-02-T2, 01-03-T3, 01-04-T3, 01-05-T2, 01-06-T1

**Approval:** approved 2026-05-06

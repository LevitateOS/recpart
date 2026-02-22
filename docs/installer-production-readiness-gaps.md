# recpart Production Readiness Audit (Fresh)

Date: 2026-02-21  
Scope lock: `tools/recpart/**` only  
Method: re-audited against `tools/recpart/REQUIREMENTS.md`, current source, current tests, and `cargo test -p recpart`

## Scope Rules Used

- This document contains only `recpart` implementation gaps.
- No cross-crate work items are tracked here.
- LOC estimates are `recpart` code + `recpart` tests + `recpart` docs only.

## Current Baseline

- `cargo test -p recpart` passes.
- Core plan/apply paths work.
- Real-disk tests exist but remain ignored by default.

## Gaps (Rebuilt From Scratch)

| Gap ID | Priority | Status | Missing Feature | Requirement Link | Evidence | Est. LOC |
|---|---|---|---|---|---|---:|
| RP-001 | P0 | Open | Implement a real 960x1080-first TUI layout (not prompt-sequence IO), including reusable screen components and no horizontal-overflow behavior at target size. | `REQ-UI-001`, `REQ-UI-002` | `tools/recpart/src/tui.rs:13` | 420-860 |
| RP-002 | P0 | Completed | Make partition-script preview mandatory before destructive apply (today it is optional prompt). | `REQ-DESIGN-011`, `REQ-UI-020` | `tools/recpart/src/tui.rs:65` | 60-150 |
| RP-003 | P0 | Completed | Add explicit destructive confirmation screen that repeats selected disk + mode at confirm time and in final summary output. | `REQ-DESIGN-002`, `REQ-UI-021` | `tools/recpart/src/tui.rs:83`, `tools/recpart/src/tui.rs:111` | 70-170 |
| RP-004 | P0 | Completed | Add execution progress view during real apply (step-by-step live progress in TUI). | `REQ-UI-020` | `tools/recpart/src/tui.rs:102`, `tools/recpart/src/tui.rs:111` | 110-260 |
| RP-005 | P1 | Completed | Replace env-only terminal size detection with robust terminal-size probing and hard-fail semantics when below minimum. | `REQ-UI-010`, `REQ-UI-011` | `tools/recpart/src/tui.rs:243`, `tools/recpart/src/tui.rs:267` | 60-140 |
| RP-006 | P0 | Completed | Add explicit disk writability/read-only preflight validation before destructive operations. | `REQ-EXEC-001` | `tools/recpart/src/preflight.rs:67`, `tools/recpart/src/exec.rs:42` | 90-220 |
| RP-007 | P1 | Completed | Harden apply sequence for real hardware reliability (`wipefs` policy, `udevadm settle`, stronger partition-device readiness semantics, failure cleanup guidance). | `REQ-EXEC-011`, `REQ-EXEC-012` | `tools/recpart/src/preflight.rs:11`, `tools/recpart/src/exec.rs:115`, `tools/recpart/src/exec.rs:403` | 200-420 |
| RP-008 | P0 | Completed | Fix AB mount topology/handoff contract so generated next steps do not produce incomplete installs (current `/state` outside `sysroot` handoff flow). | `REQ-HANDOFF-001`, `REQ-HANDOFF-010` | `tools/recpart/src/exec.rs:179`, `tools/recpart/src/handoff.rs:16` | 120-260 |
| RP-009 | P0 | Completed | Correct A/B inactive-slot semantics (`inactive_slot_hint` currently duplicates install slot). | `REQ-MODE-011`, `REQ-HANDOFF-002` | `tools/recpart/src/handoff.rs:30`, `tools/recpart/src/handoff.rs:31` | 20-70 |
| RP-010 | P1 | Completed | Expand structured apply result to include partition map + labels directly (not only device list + mounts). | `REQ-HANDOFF-010` | `tools/recpart/src/types.rs:100` | 90-190 |
| RP-011 | P1 | Completed | Add first-class user layout request inputs (CLI + planner validation) with policy defaults used only when request fields are omitted. | `REQ-MODE-010`, `REQ-MODE-020` | `tools/recpart/src/policy.rs:3`, `tools/recpart/src/cli.rs:31` | 200-420 |
| RP-012 | P0 | Completed | Implement missing conformance tests declared in requirements traceability matrix (`ui_layout`, `preflight`, `apply_flow`, `plan_only`, `cli`, `errors`, `safety_confirm`). | `REQ-CONF-001` | `tools/recpart/REQUIREMENTS.md:306`, `tools/recpart/tests/integration_cli.rs:16` | 430-900 |
| RP-013 | P1 | Completed | Repair docs/spec drift: use-case matrix still states TUI is unimplemented (`E010`). Also add explicit exit-code docs table for operators. | `REQ-ERR-001`, `REQ-CONF-001` | `tools/recpart/docs/use-cases.md:18`, `tools/recpart/src/error.rs:7` | 40-130 |

## Total Estimated Work (recpart only)

- Remaining P0 subtotal: **420 to 860 LOC**
- Remaining P1 subtotal: **0 LOC**
- Remaining overall subtotal: **420 to 860 LOC**

## Recommended First Cut (Minimal Production Candidate)

1. RP-001

Expected remaining first-cut effort: **420 to 860 LOC**

## Focused Table: Hardcoded Layout/Placement Gap

Target gap: `RP-011` (first-class user layout request instead of constants-only planning)

| Step | Work Item | Primary Files | Est. LOC |
|---|---|---|---:|
| 1 | Add first-class `LayoutProfile` + `LayoutRequest` models and serialize both requested and resolved layout in plan output. | `tools/recpart/src/types.rs`, `tools/recpart/src/plan.rs` | 60-140 |
| 2 | Refactor policy constants into `policy_defaults` profile builders (`ab`, `mutable`) and keep deterministic behavior when request fields are omitted. | `tools/recpart/src/policy.rs`, `tools/recpart/src/plan.rs` | 40-100 |
| 3 | Add CLI layout-request flags for sizing/placement (mode-safe flags + validation errors). | `tools/recpart/src/cli.rs` | 50-110 |
| 4 | Add planner validation for size constraints, missing required partitions, and sum-to-disk rules with actionable errors. | `tools/recpart/src/plan.rs`, `tools/recpart/src/error.rs` | 70-150 |
| 5 | Make mount topology derivation data-driven from plan/profile instead of fixed index assumptions where feasible. | `tools/recpart/src/exec.rs` | 60-130 |
| 6 | Include `layout_request` and `resolved_layout` in structured JSON outputs (`plan` and `apply`) for automation reproducibility. | `tools/recpart/src/types.rs`, `tools/recpart/src/cli.rs` | 30-80 |
| 7 | Add TUI layout-selection/edit step (safe defaults + explicit “advanced” edit path). | `tools/recpart/src/tui.rs` | 80-170 |
| 8 | Add dedicated tests: layout-request parsing, validation failures, deterministic output with same request, and JSON contract assertions. | `tools/recpart/tests/*` | 80-180 |
| 9 | Update recpart docs/spec/use-cases with layout-request examples and constraints. | `tools/recpart/REQUIREMENTS.md`, `tools/recpart/docs/use-cases.md` | 20-60 |

Estimated subtotal for this gap: **490 to 1,120 LOC**

# recpart Requirements

**Version:** 0.1.0  
**Status:** Draft  
**Last Updated:** 2026-02-21

This document defines the requirements for `recpart`: a partitioning and
mount-preparation wizard for LevitateOS install workflows. `recpart` is
mode-aware (`ab` default, `mutable`) and intentionally scoped to storage
layout orchestration and safe handoff to existing installation tools.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Terms](#2-terms)
3. [Design Principles](#3-design-principles)
4. [UI and UX Requirements](#4-ui-and-ux-requirements)
5. [Mode Model and Layout Contracts](#5-mode-model-and-layout-contracts)
6. [Execution and Safety Requirements](#6-execution-and-safety-requirements)
7. [CLI Interface](#7-cli-interface)
8. [Handoff Contract](#8-handoff-contract)
9. [Error Handling](#9-error-handling)
10. [Security](#10-security)
11. [Conformance and Traceability](#11-conformance-and-traceability)

---

## 1. Overview

### 1.1 Purpose

`recpart` provides a guided, deterministic, and safe partitioning flow for
LevitateOS installation, with explicit mode choice:

- `ab` (default): A/B immutable-ready disk layout
- `mutable`: classic mutable-root layout

### 1.2 Scope

This specification covers:

- Disk selection and validation
- Mode-aware partition planning
- Partition table application and filesystem creation
- Mount topology preparation for downstream installers
- Human-readable and machine-readable plan/result reporting

This specification does not cover:

- Rootfs extraction and payload installation (`recstrap`)
- fstab generation (`recfstab`)
- chroot setup (`recchroot`)
- slot state machine (`recab`)
- immutability/integrity enforcement policy (`recguard`)

### 1.3 Responsibility Boundary

**REQ-SCOPE-001**: `recpart` MUST be responsible only for partitioning,
formatting, and mount orchestration.

**REQ-SCOPE-002**: `recpart` MUST NOT implement full installation workflows or
slot commit/rollback semantics.

---

## 2. Terms

| Term | Definition |
|------|------------|
| `ab` mode | A/B immutable-ready partition model with separate slot roots. |
| `mutable` mode | Traditional single writable root model. |
| Plan | Deterministic storage layout and execution intent generated before applying changes. |
| Apply | Destructive execution of the approved plan on the target disk. |
| Mount topology | Final mounted directory layout exported for downstream tools. |

---

## 3. Design Principles

### 3.1 Explicit Intent

**REQ-DESIGN-001**: `recpart` MUST require an explicit mode choice (`ab` or
`mutable`), with `ab` as default.

**REQ-DESIGN-002**: The selected mode MUST be visible on every destructive
confirmation screen and in final output.

### 3.2 Determinism

**REQ-DESIGN-010**: Given identical inputs, `recpart` MUST generate identical
partition plans and command sequences.

**REQ-DESIGN-011**: `recpart` MUST show the exact generated partition script
before apply.

### 3.3 Fail Fast

**REQ-DESIGN-020**: `recpart` MUST stop immediately on critical command failure.

**REQ-DESIGN-021**: `recpart` MUST NOT use silent fallback behavior after a
failed destructive operation.

### 3.4 Automation-Friendly

**REQ-DESIGN-030**: Non-interactive paths MUST be available for automation.

**REQ-DESIGN-031**: Success output SHOULD be concise; failure output MUST be
actionable and explicit.

---

## 4. UI and UX Requirements

### 4.1 Primary UI Target

**REQ-UI-001**: The primary UI layout target MUST be `960x1080` (half-width
1080p design baseline).

**REQ-UI-002**: Core workflow screens MUST be readable at the primary target
without horizontal scrolling.

### 4.2 Terminal Constraints

**REQ-UI-010**: `recpart` MUST support a minimum terminal size of `80x24`.

**REQ-UI-011**: If terminal size is below minimum, `recpart` MUST fail with an
explicit message describing required dimensions.

### 4.3 Wizard Flow

**REQ-UI-020**: The wizard MUST include, at minimum:

1. Disk selection
2. Mode selection (`ab` default, `mutable`)
3. Plan preview (including generated partition script)
4. Explicit destructive confirmation
5. Execution progress view
6. Final handoff summary

**REQ-UI-021**: The destructive confirmation step MUST require deliberate user
action and clearly identify target disk and selected mode.

---

## 5. Mode Model and Layout Contracts

### 5.1 Mode Defaults

**REQ-MODE-001**: Default mode MUST be `ab` when mode is not provided.

**REQ-MODE-002**: `mutable` mode MUST be explicitly selected by user/flag.

### 5.2 A/B Layout Intent

**REQ-MODE-010**: `ab` mode MUST produce a layout that includes:

- EFI system partition
- Root slot A partition
- Root slot B partition
- Persistent writable partition(s) needed by policy

**REQ-MODE-011**: `ab` mode output MUST identify active vs inactive install
target semantics for downstream tooling.

### 5.3 Mutable Layout Intent

**REQ-MODE-020**: `mutable` mode MUST produce a layout with single root
partition semantics suitable for mutable installations.

### 5.4 Labeling and Discoverability

**REQ-MODE-030**: Plans MUST assign stable, mode-aware partition labels that
allow downstream tooling to discover intended mount targets deterministically.

---

## 6. Execution and Safety Requirements

### 6.1 Preflight Validation

**REQ-EXEC-001**: `recpart` MUST validate target disk existence and
writability before any destructive operation.

**REQ-EXEC-002**: `recpart` MUST reject execution if required host tools are
missing.

### 6.2 Apply Behavior

**REQ-EXEC-010**: Apply MUST run only after successful preflight and explicit
confirmation.

**REQ-EXEC-011**: Apply MUST execute in deterministic order:

1. Partition table write
2. Filesystem creation/labeling
3. Mount topology creation

**REQ-EXEC-012**: If any step fails, `recpart` MUST stop and print recovery
guidance.

### 6.3 Dry Run

**REQ-EXEC-020**: `recpart` MUST support a non-destructive plan mode that
prints intended changes without modifying disk state.

---

## 7. CLI Interface

### 7.1 Commands

**REQ-CLI-001**: `recpart` MUST provide:

- `recpart tui`
- `recpart plan`
- `recpart apply`

### 7.2 Mode Flags

**REQ-CLI-010**: CLI MUST accept mode selection as `ab|mutable`.

**REQ-CLI-011**: Omitting mode MUST resolve to `ab`.

### 7.3 Output Modes

**REQ-CLI-020**: `plan` and `apply` SHOULD support machine-readable output
(`--json`) for harness integration.

---

## 8. Handoff Contract

### 8.1 Downstream Integration

**REQ-HANDOFF-001**: On successful apply, `recpart` MUST print clear next-step
commands for downstream tooling (`recstrap`, `recfstab`, `recchroot`).

**REQ-HANDOFF-002**: For `ab` mode, handoff output MUST include slot-target
context needed by downstream A/B-aware tooling (`recab` and wrappers).

### 8.2 Structured Result

**REQ-HANDOFF-010**: `recpart` MUST expose a structured result object that
includes disk, mode, partition map, labels, and mount points.

---

## 9. Error Handling

### 9.1 Exit Codes

**REQ-ERR-001**: `recpart` MUST define stable, documented exit codes.

**REQ-ERR-002**: Each failure MUST identify component, failed expectation, and
concrete remediation.

### 9.2 No Masking

**REQ-ERR-010**: `recpart` MUST NOT downgrade critical failures to warning-only
success states.

---

## 10. Security

### 10.1 Destructive Safety

**REQ-SEC-001**: `recpart` MUST never perform destructive disk writes without
explicit final confirmation in interactive mode.

**REQ-SEC-002**: `recpart` MUST reject ambiguous disk targets.

### 10.2 Scope-Limited Trust

**REQ-SEC-010**: `recpart` MUST treat immutability enforcement as out of scope
and MUST delegate that responsibility to dedicated policy tools (for example,
`recguard`).

---

## 11. Conformance and Traceability

### 11.1 Requirement IDs

**REQ-CONF-001**: All behavior tests SHOULD trace back to one or more
`REQ-*` identifiers from this document.

### 11.2 Minimum Conformance Coverage

Implementations SHOULD include conformance checks for:

- Mode defaulting (`REQ-MODE-001`)
- Primary UI target behavior (`REQ-UI-001`, `REQ-UI-002`)
- Deterministic plan generation (`REQ-DESIGN-010`)
- Apply safety gates (`REQ-EXEC-001`, `REQ-EXEC-010`)
- Handoff summary correctness (`REQ-HANDOFF-001`, `REQ-HANDOFF-010`)

### 11.3 Traceability Matrix

| Requirement | Planned Test Target | Notes |
|-------------|---------------------|-------|
| `REQ-MODE-001`, `REQ-MODE-002` | `tests/mode_defaults.rs` | Validate default `ab` and explicit `mutable` selection behavior. |
| `REQ-DESIGN-010`, `REQ-DESIGN-011` | `tests/plan_determinism.rs` | Same input must produce byte-identical plan/script output. |
| `REQ-UI-001`, `REQ-UI-002`, `REQ-UI-010`, `REQ-UI-011` | `tests/ui_layout.rs` | Snapshot/layout checks for `960x1080` target and minimum terminal handling. |
| `REQ-EXEC-001`, `REQ-EXEC-002` | `tests/preflight.rs` | Missing disk/tools and invalid target rejection. |
| `REQ-EXEC-010`, `REQ-EXEC-011`, `REQ-EXEC-012` | `tests/apply_flow.rs` | Ordered execution, fail-fast behavior, and recovery guidance assertions. |
| `REQ-EXEC-020` | `tests/plan_only.rs` | Dry-run mode asserts no destructive commands executed. |
| `REQ-CLI-001`, `REQ-CLI-010`, `REQ-CLI-011`, `REQ-CLI-020` | `tests/cli.rs` | Command surface, mode flags, defaults, and JSON output contract. |
| `REQ-HANDOFF-001`, `REQ-HANDOFF-002`, `REQ-HANDOFF-010` | `tests/handoff.rs` | Next-step command output and structured result schema checks. |
| `REQ-ERR-001`, `REQ-ERR-002`, `REQ-ERR-010` | `tests/errors.rs` | Stable exit codes, actionable diagnostics, and no failure masking. |
| `REQ-SEC-001`, `REQ-SEC-002` | `tests/safety_confirm.rs` | Explicit destructive confirmation and ambiguous-target rejection. |

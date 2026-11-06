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
11. [Validation](#11-validation)

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
- immutability/integrity enforcement policy

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

`tui/apps/install/disk-plan` renders recpart's JSON as a page flow (`src/flow.ts`);
it ships on no ISO yet. recpart's part is the JSON contract it renders.

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

**REQ-HANDOFF-001**: On successful apply, `recpart` MUST report the install
target and mounts as data; the next steps belong to the installer UI and live
docs, not to recpart.

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
and MUST delegate that responsibility to dedicated policy tools.

---

## 11. Validation

Unit tests are prohibited. Choose focused validation for the behavior being changed:

- `cargo check -p recpart` for compilation.
- `cargo test -p recpart --test integration_cli` for the actual CLI and JSON contract.
- `tests/loop_disk.rs` (root) for plan, dry-run and apply on a loop device.

Use `docs/use-cases.md` for example workflows. No traceability matrix or minimum coverage gate is required during development.

# recpart

Partition planning and apply backend for LevitateOS install flows.

## Scope

`recpart` is responsible for partitioning, formatting, and mount orchestration only.
It does not perform full OS installation.

## Frontend

The canonical interactive TUI flow lives at
`tui/apps/install/disk-plan`, built on `tui/kit/core` and recpart
backend JSON contracts.

## Goals

- Provide a safe disk partition workflow for backend consumers.
- Support two layout modes:
  - `ab` (default): A/B immutable-ready partition layout.
  - `mutable`: classic mutable root layout.
- Preview exact commands/scripts before destructive actions.
- Fail fast with explicit diagnostics and remediation.

## Non-Goals

- Slot switching or commit/rollback state machine.
- Integrity policy enforcement (dm-verity, signature verification, measured boot).
- Full install automation (rootfs extraction, chroot config, service enablement).

## Planned UX

1. Select target disk.
2. Select layout mode (`ab` default, `mutable`).
3. Show partition plan and generated `sfdisk` script.
4. Explicit destructive confirmation.
5. Apply partition table.
6. Format filesystems and apply labels.
7. Mount targets under a predictable tree for downstream tools.
8. Print next commands (`recstrap`, `recfstab`, `recchroot`).

## Planned Components

- `plan` module:
  - deterministic partition plan model
  - mode-specific templates
  - `sfdisk` script generation
- `exec` module:
  - command runner with structured logs
  - idempotent safety checks

## Integration Contract

- Input: selected disk + mode + optional layout request values (policy defaults apply when omitted).
- Output: mounted target topology + machine-readable summary.
- Downstream tools:
  - `recstrap`
  - `recfstab`
  - `recchroot`
  - `recab` (A/B mode consumers)

## Error Codes

Every failure reports an `E0NN` code (also the exit status and the JSON `code`). The codes are defined once, in [`src/error.rs`](src/error.rs).


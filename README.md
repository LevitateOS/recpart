# recpart

Partitioning wizard for LevitateOS install flows.

## Scope

`recpart` is responsible for partitioning, formatting, and mount orchestration only.
It does not perform full OS installation.

## Goals

- Provide a safe TUI-guided disk partition workflow.
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
- `tui` module:
  - reusable wizard screens
  - confirmation and error panels

## Integration Contract

- Input: selected disk + mode + optional sizing overrides.
- Output: mounted target topology + machine-readable summary.
- Downstream tools:
  - `recstrap`
  - `recfstab`
  - `recchroot`
  - `recab` (A/B mode consumers)

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

- Input: selected disk + mode + optional layout request values (policy defaults apply when omitted).
- Output: mounted target topology + machine-readable summary.
- Downstream tools:
  - `recstrap`
  - `recfstab`
  - `recchroot`
  - `recab` (A/B mode consumers)

## Exit Codes

| Code | Meaning |
|---|---|
| `1` (`E001`) | Invalid target disk / target safety failure |
| `2` (`E002`) | Required tool missing |
| `3` (`E003`) | Plan generation or policy validation failure |
| `4` (`E004`) | Missing destructive confirmation |
| `5` (`E005`) | Partition apply failure |
| `6` (`E006`) | Filesystem format failure |
| `7` (`E007`) | Mount validation or mount execution failure |
| `8` (`E008`) | Handoff generation failure |
| `9` (`E009`) | JSON serialization failure |
| `10` (`E010`) | Reserved not-implemented code |
| `11` (`E011`) | Root privileges required |
| `12` (`E012`) | Internal/runtime error |

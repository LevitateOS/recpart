# recpart

Partition planning and apply backend for LevitateOS install flows.

## Scope

`recpart` is responsible for partitioning, formatting, and mount orchestration only.
It does not perform full OS installation.

## Frontend

An interactive flow over recpart's JSON lives at `tui/apps/install/disk-plan`;
both live ISOs ship it as the `disk-plan` command.

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

## How It Works

`plan` derives the partition templates (filesystem, label, installed mount
point) for the mode and the `sfdisk` script; `apply` runs exactly those:
partition, format, then mount at `<mount-root>/sysroot`, and reports a
structured handoff (install target, mounts, A/B slot devices). `apply
--dry-run` lists the steps and what apply would refuse. The installer's next
steps (recstrap, recfstab, recchroot) are the installer UI's and live docs'.

## Integration Contract

- Input: selected disk + mode + optional layout request values (policy defaults apply when omitted).
- Output: mounted target topology + machine-readable summary (JSON Schemas in
  `docs/frontend-handoff/schemas/`, generated from `src/types.rs`).

## Error Codes

Every failure reports an `E0NN` code (also the exit status and the JSON `code`). The codes are defined once, in [`src/error.rs`](src/error.rs).


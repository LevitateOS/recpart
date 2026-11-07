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

`inspect --disk <disk>` reports what is on the disk: the table (GPT, MBR or
none), every partition (number, start, size, type GUID, name, filesystem,
filesystem label and UUID, used space, mounts, and the sizes recpart could
resize it to, or why it can't: XFS only grows, swap holds nothing to keep,
a mounted filesystem isn't resized) and the free gaps. recpart resizes ext2/3/4,
FAT and NTFS, unmounted. Used space of an unmounted filesystem needs root
(dumpe2fs, `fsck.fat -n`, `ntfsresize --info`).

`plan` builds the layout one of two ways:

- **A template** (`--mode ab|mutable`, sizes from `--efi-size-mb` and the
  like): the whole disk is erased and gets the mode's partitions.
- **An explicit layout** (`--layout <file>`, `-` for stdin): actions on the
  disk `inspect` reported. Partitions no action names stay untouched.

```json
{
  "table_id": "7D3B1C5E-2A4F-4E8B-9C61-0F2E8A4D5B10",
  "actions": [
    { "action": "keep", "partition": 1, "role": "efi" },
    { "action": "resize", "partition": 3, "size_mb": 102400 },
    { "action": "create", "role": "root_a", "size_mb": 20480 },
    { "action": "create", "role": "root_b", "size_mb": 20480 },
    { "action": "create", "role": "swap", "size_mb": 4096 },
    { "action": "create", "role": "state" }
  ]
}
```

  Actions: `keep`, `delete`, `create` (`size_mb`, or the rest of a gap
  without it; `start_mb` picks the gap), `resize` (with its filesystem,
  keeping the start; without `size_mb` it grows into the gap after it) and
  `format` (`filesystem` `ext4`, `vfat` or `swap`, and `label`). `role` says
  what the installed system uses a partition for: `efi` (/boot), `root`,
  `root_a`, `root_b`, `state` (/var), `home`, `swap`, or `other` with a
  `mountpoint`. `new_table: true` replaces the table with an empty GPT one;
  `table_id` makes the plan refuse a disk whose table changed; `swap_file`
  (`size_mb`, `location` `root` or `var`) adds a swap file. Sizes are MiB.
  The schema is `docs/frontend-handoff/schemas/layout.schema.json`.

  The layout is checked against the mode: `ab` needs one each of `efi`,
  `root_a`, `root_b` (the same size) and `state`; `mutable` needs `efi` and
  `root`. Roots and state are always formatted; the minimum sizes, mount
  points used twice, 4 KiB alignment, and the EFI partition (FAT, an EFI
  system partition, with room for the mode's kernels: 128 MiB per slot plus
  8 MiB) are checked too. A kept EFI partition (dual boot) is mounted, not
  formatted. Resizes: ext4 (`e2fsck -f` + `resize2fs`, offline), FAT
  (`fatresize`) and NTFS (`ntfsresize`, which refuses a hibernated or unclean
  Windows volume); a shrink keeps the used space plus 10% and 64 MiB.

Either way the plan holds the exact `sfdisk` script (an explicit layout's
writes every partition back with its own start, size, type, UUID, name and
attributes), the partitions the installed system uses, and `summary`: every
partition's change (`erased`, `created`, `resized`, `formatted`, `untouched`)
with its state before and after, for the confirm page. `apply` runs it:
shrink, write the table (after `wipefs -a` when the whole disk is erased),
grow, format, mount at `<mount-root>/sysroot`, then `swapon` the swap
partitions and swap file so recfstab finds them; it reports a structured
handoff (install target, mounts, swaps, A/B slot devices). `apply --dry-run`
lists the steps and what apply would refuse. The installer's next steps
(recstrap, recfstab, recchroot) are the installer UI's and live docs'.

`--inventory <file>` plans and dry-runs against a recorded `inspect --json`
document instead of the disk (the live-disk checks are skipped and the dry
run says so); a real apply refuses it. disk-plan's stand-in
(`tui/apps/install/disk-plan/fixtures`) records its documents this way.

## Integration Contract

- Input: selected disk + mode + optional layout request values (policy
  defaults apply when omitted), or an explicit layout.
- Output: mounted target topology + machine-readable summary (JSON Schemas in
  `docs/frontend-handoff/schemas/`, generated from `src/types.rs`).

## Error Codes

Every failure reports an `E0NN` code (also the exit status and the JSON `code`) and a remediation. The codes are defined once, in [`src/error.rs`](src/error.rs); E014 to E026 cover the explicit layout, resizes and swap.


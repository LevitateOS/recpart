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

`signatures` lists what is written on the whole disk itself, outside any
partition: a LUKS container, a filesystem or an LVM PV on the bare disk, which
shows as table `none` with no partitions. Each is
`{ "kind": "crypto_LUKS", "offset_bytes": 0, "uuid": "...", "label": null }`
(`kind` is wipefs's TYPE: `crypto_LUKS`, `btrfs`, `LVM2_member`, ...). They
come from `wipefs --no-act --json` (root); without it, from lsblk's FSTYPE of
the disk, with `offset_bytes` 0. The table's own signatures (`gpt`, `PMBR`,
`dos`) are not listed. A recorded document without the field reads as none.

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
  `mountpoint`. `new_table: true` replaces the table with an empty GPT one
  (a disk with no table gets one either way, but one holding `signatures`
  needs `new_table` said explicitly, E014);
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
with its state before and after, for the confirm page. It names the disk it
was made for: `disk` (path, size, sector sizes), `disk_serial` and `table_id`
(`inspect`'s). When it erases the whole disk (`wipes_disk`: a template, or
`new_table`), `erased_signatures` lists the disk's `signatures` that go with
it, so the confirm page can say "a LUKS container on the whole disk is
erased"; otherwise it is empty. `apply` runs it:
shrink, write the table (after `wipefs -a` when the whole disk is erased),
grow, format, mount at `<mount-root>` (`/mnt` by default), then `swapon` the swap
partitions and swap file so recfstab finds them; it reports a structured
handoff (install target, mounts, swaps, A/B slot devices). `apply --dry-run`
lists the steps and what apply would refuse.

`apply` plans again from the disk as it is now. `--expect <file>` ties it to
the plan the user confirmed: the file is what `plan --json` printed for the
same arguments, and before anything runs (dry run too) apply compares the new
plan with it as JSON values (whitespace and key order don't matter). Any
difference refuses with E023, naming the first that differs: the disk's
serial, path or size, its table id, its whole-disk signatures, the mode or
layout arguments, the partitions (`summary`), or another plan field. A file
that is not a plan document is E027. `--expect` is optional for scripts, but
frontends must pass it: without it a disk swapped since the confirm page (a
USB stick plugged in, `/dev/sdb` now another disk) is what gets erased.

The installer's next steps (recstrap, recfstab, recchroot) are the installer
UI's and live docs'.

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

Every failure reports an `E0NN` code (also the exit status and the JSON `code`) and a remediation. The codes are defined once, in [`src/error.rs`](src/error.rs); E014 to E026 cover the explicit layout, resizes and swap; E023 also covers `apply --expect` finding a different plan, and E027 an unreadable `--expect` file.


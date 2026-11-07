# recpart Frontend Handoff

This directory defines the backend contract frontend/TUI consumers should use.

## Contract Policy

- Backend emits schema-versioned JSON payloads.
- Frontend MUST treat unknown required fields as incompatibility.
- Additive optional fields may appear without a schema major bump.
- Breaking changes require a schema major bump.

## Runtime Flows

1. `list-disks` flow:
- Request disk inventory (`recpart list-disks --json`)
- Render selectable disk cards from backend-provided metadata

2. `inspect` flow (manual layouts):
- Request the disk's table, partitions and free gaps (`recpart inspect --disk <disk> --json`)
- Render the partition table; `min_size_bytes`/`max_size_bytes` bound a resize

3. `plan` flow:
- Request plan output (`recpart plan --json ...`, with `--layout <file>` for
  an explicit layout, `schemas/layout.schema.json`)
- Render partition summary + script preview; a refusal is an error document
  with its code and remediation

4. `apply` flow:
- Request dry run first (`recpart apply --dry-run --json ...`)
- Show steps + destructive confirmation UI, built from the plan's `summary`
  (erased, created, resized, formatted, untouched)
- Execute real apply with confirmation token

## Primary Files

- `schemas/*.schema.json` - JSON Schemas of `list-disks`, `inspect`, `plan`,
  `apply` and error output, and of the `--layout` input, generated from recpart's output types (`src/types.rs`) by
  `cargo xtask gen` in the LevitateOS workspace, which also writes the
  disk-plan TUI's TypeScript declarations. Do not edit them by hand.

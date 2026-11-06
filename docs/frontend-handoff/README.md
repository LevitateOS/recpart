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

2. `plan` flow:
- Request plan output (`recpart plan --json ...`)
- Render partition summary + script preview

3. `apply` flow:
- Request dry run first (`recpart apply --dry-run --json ...`)
- Show steps + destructive confirmation UI
- Execute real apply with confirmation token

## Primary Files

- `schemas/*.schema.json` - JSON Schemas of `list-disks`, `plan`, `apply` and
  error output, generated from recpart's output types (`src/types.rs`) by
  `cargo xtask gen` in the LevitateOS workspace, which also writes the
  disk-plan TUI's TypeScript declarations. Do not edit them by hand.

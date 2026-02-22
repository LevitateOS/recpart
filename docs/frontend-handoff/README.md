# recpart Frontend Handoff

This directory defines the backend contract frontend/TUI consumers should use.

## Contract Policy

- Backend emits schema-versioned JSON payloads.
- Frontend MUST treat unknown required fields as incompatibility.
- Additive optional fields may appear without a schema major bump.
- Breaking changes require a schema major bump.

## Runtime Flows

1. `plan` flow:
- Request plan output (`recpart plan --json ...`)
- Render partition summary + script preview

2. `apply` flow:
- Request dry run first (`recpart apply --dry-run --json ...`)
- Show steps + destructive confirmation UI
- Execute real apply with confirmation token

## Primary Files

- `state-machine.md` - backend state/event contract for UI orchestration
- `schemas/plan-result.schema.json`
- `schemas/apply-result.schema.json`
- `schemas/error.schema.json`
- `examples/*.json` - sample payloads for frontend development

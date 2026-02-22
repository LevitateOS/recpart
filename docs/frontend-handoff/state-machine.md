# Backend State Machine Contract

Frontend should model recpart backend interactions with this state machine.

## States

- `idle`
- `planning`
- `plan_ready`
- `apply_dry_running`
- `apply_dry_ready`
- `awaiting_confirm`
- `applying`
- `apply_done`
- `failed`

## Events

- `plan_requested`
- `plan_succeeded`
- `plan_failed`
- `apply_dry_requested`
- `apply_dry_succeeded`
- `apply_dry_failed`
- `confirm_submitted`
- `apply_requested`
- `apply_succeeded`
- `apply_failed`

## Required UI Behaviors

- Always run a dry-run apply before destructive apply.
- Display backend-provided step list exactly as emitted.
- Display remediation text from backend errors verbatim.
- Require explicit destructive confirmation token before apply.

## Error Handling Rules

- Any non-zero backend exit transitions to `failed`.
- Frontend should parse `error.schema.json` payload when JSON mode is used.
- If JSON parse fails, fallback to raw stderr display.

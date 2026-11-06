# recpart Use-Case Scenarios

This document defines end-user and automation scenarios for `recpart` backend usage.

## Scenario Matrix

| ID | Scenario | Mode | Command Path | Expected Outcome | Test Mapping |
|----|----------|------|--------------|------------------|--------------|
| UC-001 | Generate partition plan for throwaway disk | `ab` | `recpart plan --disk <disk> --json` | Plan JSON emitted with schema version and 4 AB partitions. | `tests/e2e_real_disk.rs::e2e_plan_on_real_disk_json_contract` |
| UC-002 | Dry-run AB apply for preview-only validation | `ab` | `recpart apply --disk <disk> --mode ab --dry-run --json` | Step list emitted, no destructive write, AB handoff present. | `tests/e2e_real_disk.rs::e2e_apply_dry_run_on_real_disk_json_contract` |
| UC-003 | Destructive AB apply on throwaway disk | `ab` | `recpart apply --disk <disk> --mode ab --confirm DESTROY --json` | Partition/format/mount succeed and mount topology is available. | `tests/e2e_real_disk.rs::e2e_apply_destructive_ab_mode` (ignored by default) |
| UC-004 | Generate partition plan for mutable workflow | `mutable` | `recpart plan --disk <disk> --mode mutable --json` | Plan JSON emitted with EFI + ROOT only. | Manual CLI workflow |
| UC-005 | Dry-run mutable apply for automation | `mutable` | `recpart apply --disk <disk> --mode mutable --dry-run --json` | Step list emitted, mutable handoff has no slot context. | Manual CLI workflow |
| UC-006 | Reject invalid target path | N/A | `recpart plan --disk /dev/null --json` | Non-zero exit, structured JSON error `E001`. | `tests/integration_cli.rs::plan_json_error_contract_for_invalid_disk` |
| UC-007 | Reject protected mount root | any | `recpart apply ... --dry-run --mount-root /` | Non-zero result with mount safety error. | Manual CLI workflow |
| UC-008 | Require explicit confirmation for destructive apply | any | `recpart apply ...` (without `--confirm DESTROY`) | Non-zero result with `E004`. | Manual CLI workflow |
| UC-009 | Preserve deterministic plan output | any | repeat same `plan` command | byte-identical plan JSON/script output | Manual CLI workflow |

## How To Run Scenario Tests

### Standard (non-destructive)

```bash
cargo test -p recpart --test integration_cli
```

### Real-disk non-destructive e2e

```bash
RECPART_E2E_DISK=/dev/<throwaway-disk> \
cargo test -p recpart --test e2e_real_disk -- --ignored \
  e2e_plan_on_real_disk_json_contract \
  e2e_apply_dry_run_on_real_disk_json_contract
```

### Real-disk destructive e2e (explicit opt-in)

```bash
sudo RECPART_E2E_DISK=/dev/<throwaway-disk> \
RECPART_E2E_ALLOW_DESTRUCTIVE=YES_DESTROY \
cargo test -p recpart --test e2e_real_disk -- --ignored \
  e2e_apply_destructive_ab_mode
```

## Notes

- Destructive scenario UC-003 is intentionally ignored by default.
- All automation paths should prefer JSON outputs and parse schema-versioned payloads.
- Frontend teams should consume scenario expectations together with:
  - `docs/frontend-handoff/schemas/*.json`

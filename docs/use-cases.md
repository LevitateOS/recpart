# recpart Use-Case Scenarios

This document defines end-user and automation scenarios for `recpart` backend usage.

## Scenario Matrix

| ID | Scenario | Mode | Command Path | Expected Outcome |
|----|----------|------|--------------|------------------|
| UC-001 | Generate partition plan for throwaway disk | `ab` | `recpart plan --disk <disk> --json` | Plan JSON emitted with schema version and 4 AB partitions. |
| UC-002 | Dry-run AB apply for preview-only validation | `ab` | `recpart apply --disk <disk> --mode ab --dry-run --json` | Step list emitted, no destructive write, AB handoff present. |
| UC-003 | Destructive AB apply on throwaway disk | `ab` | `recpart apply --disk <disk> --mode ab --confirm DESTROY --json` | Partition/format/mount succeed and mount topology is available. |
| UC-004 | Generate partition plan for mutable workflow | `mutable` | `recpart plan --disk <disk> --mode mutable --json` | Plan JSON emitted with EFI + ROOT only. |
| UC-005 | Dry-run mutable apply for automation | `mutable` | `recpart apply --disk <disk> --mode mutable --dry-run --json` | Step list emitted, mutable handoff has no slot context. |
| UC-006 | Reject invalid target path | N/A | `recpart plan --disk /dev/null --json` | Non-zero exit, structured JSON error `E001`. |
| UC-007 | Reject protected mount root | any | `recpart apply ... --dry-run --mount-root /` | Non-zero result with mount safety error. |
| UC-008 | Require explicit confirmation for destructive apply | any | `recpart apply ...` (without `--confirm DESTROY`) | Non-zero result with `E004`. |
| UC-009 | Preserve deterministic plan output | any | repeat same `plan` command | byte-identical plan JSON/script output |

## Checks

```bash
cargo test -p recpart --test integration_cli          # UC-006, unprivileged
cargo test -p recpart --no-run                        # then, as root:
sudo target/debug/deps/loop_disk-<hash> --include-ignored   # UC-001..UC-003 on a loop device
```

`tests/loop_disk.rs` plans, dry-runs and applies the A/B layout on a sparse
loop device and checks labels, mounts and the handoff; no real disk is
touched.

## Notes

- All automation paths should prefer JSON outputs and parse schema-versioned payloads.
- Frontend teams should consume scenario expectations together with:
  - `docs/frontend-handoff/schemas/*.json`

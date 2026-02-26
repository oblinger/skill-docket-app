# Integration Check

Verify that components Alpha and Beta integrate correctly. This task is assigned to the checker agent (hwc1).

## Steps

1. Receive assignment from PM
2. Verify `02_build_component_alpha/alpha.built` exists and is valid
3. Verify `03_build_component_beta/beta.built` exists and is valid
4. Create integration verification artifact: `integration.verified` in this folder
5. The verification file should contain: checker agent name, timestamp, and references to both component artifacts
6. Mark this task complete
7. Report verification result to PM

## Acceptance Criteria

- Both `alpha.built` and `beta.built` exist and contain valid data
- `integration.verified` file exists in `04_integration_check/`
- Task status is `completed`
- PM has been notified of pass/fail result

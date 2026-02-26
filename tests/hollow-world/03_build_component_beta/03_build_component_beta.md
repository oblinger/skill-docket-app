# Build Component Beta

Build the second synthetic component of the hollow world project. This task is assigned to a builder agent (hwb2).

## Steps

1. Receive assignment from PM
2. Create the Beta component artifact: a marker file `beta.built` in this folder
3. The marker file should contain: agent name, timestamp, and "Component Beta built successfully"
4. Mark this task complete
5. Report completion to PM

## Acceptance Criteria

- `beta.built` file exists in `03_build_component_beta/`
- File contains valid agent name and timestamp
- Task status is `completed`
- PM has been notified

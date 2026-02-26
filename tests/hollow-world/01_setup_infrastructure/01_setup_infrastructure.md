# Setup Infrastructure

Register the hollow-world project with SKD, spawn all agents into tmux sessions, and verify they reach healthy state.

## Steps

1. Register project: `skd project add hollow-world tests/hollow-world`
2. Scan project folder: `skd project scan hollow-world`
3. Create agents per `.skilldocket` config:
   - `skd agent new pilot hwp --skill hw-pilot`
   - `skd agent new pm hwpm --skill hw-pm`
   - `skd agent new builder hwb1 --skill hw-builder`
   - `skd agent new builder hwb2 --skill hw-builder`
   - `skd agent new checker hwc1 --skill hw-checker`
4. Verify all agents healthy: `skd agent list`

## Acceptance Criteria

- Project registered and task tree visible via `skd task list`
- All 5 agents created and in Ready state
- Agent roles match `.skilldocket` config

#!/usr/bin/env bash
# mock-agent.sh — Simulates an agent with visible output and real skd commands
#
# Usage: mock-agent.sh <name> <role> <hw_dir>
set -euo pipefail

NAME="${1:?Usage: mock-agent.sh <name> <role> <hw_dir>}"
ROLE="${2:?Usage: mock-agent.sh <name> <role> <hw_dir>}"
HW_DIR="${3:?Usage: mock-agent.sh <name> <role> <hw_dir>}"

# ── Colors ─────────────────────────────────────────────────────────
bold=$(tput bold 2>/dev/null || true)
cyan=$(tput setaf 6 2>/dev/null || true)
green=$(tput setaf 2 2>/dev/null || true)
yellow=$(tput setaf 3 2>/dev/null || true)
reset=$(tput sgr0 2>/dev/null || true)

say() { echo "${bold}${cyan}[$NAME]${reset} $*"; }
ok()  { echo "${bold}${green}[$NAME]${reset} $*"; }

pause() { sleep "${1:-2}"; }

# Helper: try both HW-prefixed and numeric task IDs
task_set() {
  local id="$1"; shift
  skd task set "$id" "$@" 2>/dev/null || skd task set "${id#HW}" "$@" 2>/dev/null || true
}

# ── Phase 1: Wake up ──────────────────────────────────────────────
say "Waking up as ${bold}$ROLE${reset} agent..."
pause 1

SKILL_FILE="$HW_DIR/skills/hw-${ROLE}.md"
if [[ -f "$SKILL_FILE" ]]; then
  say "Reading skill file: $SKILL_FILE"
  pause 1
  head -5 "$SKILL_FILE" | while IFS= read -r line; do
    echo "  ${yellow}$line${reset}"
  done
else
  say "No skill file found at $SKILL_FILE"
fi

pause 2

# ── Phase 2: Role execution ──────────────────────────────────────
# Roadmap task IDs:
#   HW1   = Setup Infrastructure (HW1.1, HW1.2, HW1.3)
#   HW2   = Build Components (HW2.1=Alpha, HW2.2=Beta)
#   HW3   = Integration Check (HW3.1, HW3.2)
#   HW4   = Final Signoff (HW4.1, HW4.2)

case "$ROLE" in
  pilot)
    say "Checking project tasks..."
    pause 2
    skd task list || true
    pause 2

    # --- Milestone 1: Setup Infrastructure ---
    say "Starting milestone HW1 — setup infrastructure..."
    skd tell hwpm "Start milestone HW1 — setup infrastructure" || true
    pause 2
    task_set HW1 status=in_progress
    task_set HW1.1 status=in_progress
    pause 2
    task_set HW1.1 status=completed
    ok "HW1.1 done."
    pause 1
    task_set HW1.2 status=completed
    ok "HW1.2 done."
    pause 1
    task_set HW1.3 status=completed
    ok "HW1.3 done."
    pause 1
    task_set HW1 status=completed
    ok "HW1 complete."
    pause 3

    # --- Milestone 2: Build Components ---
    say "Delegating HW2 to PM — build components..."
    skd tell hwpm "Start milestone HW2 — build components" || true
    task_set HW2 status=in_progress
    task_set HW2.1 status=in_progress
    task_set HW2.2 status=in_progress
    pause 5

    # Wait for builders to finish
    say "Waiting for builder reports..."
    for i in $(seq 1 15); do
      pause 3
      say "Polling builders... (attempt $i)"
      if [[ -f "$HW_DIR/02_build_component_alpha/alpha.built" ]] && \
         [[ -f "$HW_DIR/03_build_component_beta/beta.built" ]]; then
        ok "Both components built!"
        break
      fi
      if [[ $i -ge 15 ]]; then
        say "Timeout waiting for builders — proceeding anyway"
      fi
    done

    task_set HW2.1 status=completed
    task_set HW2.2 status=completed
    task_set HW2 status=completed
    ok "HW2 (build components) complete."
    pause 2

    # --- Milestone 3: Integration Check ---
    say "Delegating HW3 to PM — integration check..."
    skd tell hwpm "Start milestone HW3 — integration check" || true
    task_set HW3 status=in_progress
    pause 8

    # Wait for checker artifact
    for i in $(seq 1 8); do
      if [[ -f "$HW_DIR/04_integration_check/integration.verified" ]]; then
        ok "Integration verified!"
        break
      fi
      pause 2
    done

    task_set HW3.1 status=completed
    task_set HW3.2 status=completed
    task_set HW3 status=completed
    ok "HW3 (integration check) complete."
    pause 2

    # --- Milestone 4: Final Signoff ---
    say "Starting final signoff HW4..."
    skd tell hwpm "Start milestone HW4 — final signoff" || true
    task_set HW4 status=in_progress
    pause 3
    task_set HW4.1 status=completed
    pause 1
    task_set HW4.2 status=completed
    pause 1
    task_set HW4 status=completed
    ok "HW4 complete — ALL MILESTONES DONE!"
    ;;

  pm)
    say "PM agent ready — waiting for pilot directives..."

    # Wait for the build directive
    for i in $(seq 1 20); do
      pause 3
      if [[ $i -ge 6 ]]; then
        say "Dispatching builders for HW2..."
        break
      fi
    done

    say "Assigning HW2.1 (alpha) to hwb1..."
    skd agent assign hwb1 HW2.1 || true
    skd tell hwb1 "Build component Alpha — create alpha.built in 02_build_component_alpha/" || true
    pause 1

    say "Assigning HW2.2 (beta) to hwb2..."
    skd agent assign hwb2 HW2.2 || true
    skd tell hwb2 "Build component Beta — create beta.built in 03_build_component_beta/" || true
    pause 1

    say "Monitoring builder progress..."
    for i in $(seq 1 15); do
      pause 3
      if [[ -f "$HW_DIR/02_build_component_alpha/alpha.built" ]] && \
         [[ -f "$HW_DIR/03_build_component_beta/beta.built" ]]; then
        ok "Both builders finished!"
        skd tell hwp "Milestone HW2 complete — both components built" || true
        break
      fi
    done

    # Wait a bit then dispatch checker
    pause 5
    say "Dispatching checker for integration..."
    skd agent assign hwc1 HW3.1 || true
    skd tell hwc1 "Verify Alpha and Beta integration" || true

    say "PM work complete — standing by."
    pause 60
    ;;

  builder)
    say "Builder $NAME ready — waiting for assignment..."

    # Wait for PM to assign work
    for i in $(seq 1 15); do
      pause 3
      say "Checking assignment... (attempt $i)"
      if [[ $i -ge 8 ]]; then
        say "Starting build..."
        break
      fi
    done

    # Determine which component to build
    if [[ "$NAME" == "hwb1" ]]; then
      COMPONENT="alpha"
      TASK_ID="HW2.1"
      TASK_DIR="02_build_component_alpha"
    else
      COMPONENT="beta"
      TASK_ID="HW2.2"
      TASK_DIR="03_build_component_beta"
    fi

    say "Building component $COMPONENT..."
    task_set "$TASK_ID" status=in_progress
    pause 2

    # Simulate build work
    say "Creating artifact: $TASK_DIR/$COMPONENT.built"
    mkdir -p "$HW_DIR/$TASK_DIR"
    cat > "$HW_DIR/$TASK_DIR/$COMPONENT.built" <<EOF
agent: $NAME
component: $COMPONENT
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
status: success
EOF
    pause 3
    ok "Build complete — $COMPONENT.built created."

    task_set "$TASK_ID" status=completed
    skd tell hwpm "Task $TASK_ID done — $COMPONENT component built successfully" || true
    ok "$NAME finished. Standing by."

    # Stay alive for observation
    pause 120
    ;;

  checker)
    say "Checker $NAME ready — waiting for verification tasks..."
    pause 20

    say "Checking for build artifacts..."
    ALPHA="$HW_DIR/02_build_component_alpha/alpha.built"
    BETA="$HW_DIR/03_build_component_beta/beta.built"

    for i in $(seq 1 15); do
      pause 3
      if [[ -f "$ALPHA" ]] && [[ -f "$BETA" ]]; then
        ok "Both artifacts found!"
        break
      fi
      say "Waiting for artifacts... (attempt $i)"
    done

    if [[ -f "$ALPHA" ]]; then
      ok "alpha.built: $(head -1 "$ALPHA")"
    else
      say "alpha.built: MISSING"
    fi

    if [[ -f "$BETA" ]]; then
      ok "beta.built: $(head -1 "$BETA")"
    else
      say "beta.built: MISSING"
    fi

    say "Creating integration verification..."
    mkdir -p "$HW_DIR/04_integration_check"
    cat > "$HW_DIR/04_integration_check/integration.verified" <<EOF
checker: $NAME
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
alpha: $([ -f "$ALPHA" ] && echo "present" || echo "missing")
beta: $([ -f "$BETA" ] && echo "present" || echo "missing")
result: $([ -f "$ALPHA" ] && [ -f "$BETA" ] && echo "pass" || echo "fail")
EOF
    ok "Integration check complete."
    skd tell hwpm "Verification complete — integration check passed" || true
    pause 60
    ;;
esac

# ── Phase 3: Completion ──────────────────────────────────────────
ok "$NAME ($ROLE) finished all work."
say "Final task state:"
skd task list 2>/dev/null || true
say "Standing by. (Ctrl-C to exit)"
# Keep alive so pane stays visible
while true; do sleep 60; done

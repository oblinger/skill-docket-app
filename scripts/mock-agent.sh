#!/usr/bin/env bash
# mock-agent.sh — Signal-driven mock agent with visual output
#
# Usage: mock-agent.sh <name> <role> <hw_dir> [--signals-dir <dir>] [--wait-for-start]
set -euo pipefail

NAME="${1:?Usage: mock-agent.sh <name> <role> <hw_dir> [--signals-dir <dir>] [--wait-for-start]}"
ROLE="${2:?}"
HW_DIR="${3:?}"
shift 3

# Parse optional flags
SIGNALS_DIR="/tmp/demo1-signals"
WAIT_FOR_START=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --signals-dir) SIGNALS_DIR="$2"; shift 2 ;;
    --wait-for-start) WAIT_FOR_START=true; shift ;;
    *) shift ;;
  esac
done

# ── Extract prefix and derive peer names ─────────────────────────
PREFIX=""
for suffix in pilot pm worker1 worker2 checker; do
  if [[ "$NAME" == *"$suffix" ]]; then
    PREFIX="${NAME%"$suffix"}"
    break
  fi
done

PEER_PILOT="${PREFIX}pilot"
PEER_PM="${PREFIX}pm"
PEER_WORKER1="${PREFIX}worker1"
PEER_WORKER2="${PREFIX}worker2"
PEER_CHECKER="${PREFIX}checker"

# ── Colors (role-specific) ───────────────────────────────────────
bold=$(tput bold 2>/dev/null || true)
reset=$(tput sgr0 2>/dev/null || true)
blue=$(tput setaf 4 2>/dev/null || true)
magenta=$(tput setaf 5 2>/dev/null || true)
green=$(tput setaf 2 2>/dev/null || true)
yellow=$(tput setaf 3 2>/dev/null || true)
cyan=$(tput setaf 6 2>/dev/null || true)
white=$(tput setaf 7 2>/dev/null || true)

case "$ROLE" in
  pilot)   color="$blue" ;;
  pm)      color="$magenta" ;;
  builder)
    if [[ "$NAME" == *worker1 ]]; then color="$green"
    else color="$yellow"; fi
    ;;
  checker) color="$cyan" ;;
  *)       color="$white" ;;
esac

# ── Output helpers ───────────────────────────────────────────────
say()    { echo "${bold}${color}[$NAME]${reset} $*"; }
ok()     { echo "${bold}${color}[$NAME]${reset} ${green}+${reset} $*"; }
send_msg() { echo "${bold}${color}[$NAME]${reset} ${bold}>>>${reset} Sending to ${bold}$1${reset}: \"$2\""; }
recv_msg() { echo "${bold}${color}[$NAME]${reset} ${bold}<<<${reset} Received signal: ${bold}$1${reset}"; }

banner() {
  local msg="$1"
  local width=$(( ${#msg} + 4 ))
  local line
  line=$(printf '%0.s-' $(seq 1 "$width"))
  echo ""
  echo "${bold}${color}+${line}+${reset}"
  echo "${bold}${color}|  $msg  |${reset}"
  echo "${bold}${color}+${line}+${reset}"
  echo ""
}

progress_bar() {
  local pct="$1" label="$2"
  local filled=$(( pct / 10 ))
  local empty=$(( 10 - filled ))
  local bar=""
  [[ $filled -gt 0 ]] && bar=$(printf '#%.0s' $(seq 1 "$filled"))
  [[ $empty -gt 0 ]] && bar+=$(printf -- '-%.0s' $(seq 1 "$empty"))
  echo "${bold}${color}[$NAME]${reset} [${bar}] ${pct}% -- $label"
}

# ── Signal helpers ───────────────────────────────────────────────
wait_signal() {
  local sig="$1"
  while [[ ! -f "$SIGNALS_DIR/$sig" ]]; do
    sleep 0.5
  done
  recv_msg "$sig"
}

send_signal() {
  local sig="$1"
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) from=$NAME" > "$SIGNALS_DIR/$sig"
}

# Helper: try both HW-prefixed and numeric task IDs
task_set() {
  local id="$1"; shift
  skd task set "$id" "$@" 2>/dev/null || skd task set "${id#HW}" "$@" 2>/dev/null || true
}

# ── Phase 1: Wake up ────────────────────────────────────────────
say "Waking up as ${bold}$ROLE${reset} agent..."
sleep 0.5

SKILL_FILE="$HW_DIR/skills/hw-${ROLE}.md"
if [[ -f "$SKILL_FILE" ]]; then
  say "Reading skill file: hw-${ROLE}.md"
  sleep 0.5
fi

# ── Wait for start (pilot only) ─────────────────────────────────
if [[ "$WAIT_FOR_START" == true ]]; then
  echo ""
  echo "${bold}${color}+========================================+${reset}"
  echo "${bold}${color}|  Type 'start' to begin the demo...    |${reset}"
  echo "${bold}${color}+========================================+${reset}"
  echo ""
  while true; do
    read -r input
    if [[ "$input" == "start" ]]; then
      break
    fi
    say "Unknown command: '$input' -- type 'start' to begin"
  done
  echo ""
  banner "DEMO STARTED"
fi

# ── Phase 2: Role execution ─────────────────────────────────────
case "$ROLE" in
  pilot)
    say "Checking project tasks..."
    skd task list 2>/dev/null || true
    sleep 0.5

    # --- Milestone 1: Setup Infrastructure ---
    banner "HW1 -- Setup Infrastructure"
    send_msg "$PEER_PM" "Start milestone HW1 -- setup infrastructure"
    skd tell "$PEER_PM" "Start milestone HW1 -- setup infrastructure" || true
    send_signal "hw1-start"

    task_set HW1 status=in_progress
    sleep 0.5

    say "Setting up development environment..."
    task_set HW1.1 status=in_progress
    sleep 1
    task_set HW1.1 status=completed
    ok "HW1.1 -- Dev environment ready"

    sleep 0.3
    say "Configuring CI pipeline..."
    task_set HW1.2 status=in_progress
    sleep 1
    task_set HW1.2 status=completed
    ok "HW1.2 -- CI pipeline configured"

    sleep 0.3
    say "Provisioning test fixtures..."
    task_set HW1.3 status=in_progress
    sleep 1
    task_set HW1.3 status=completed
    ok "HW1.3 -- Test fixtures ready"

    task_set HW1 status=completed
    ok "HW1 complete!"
    sleep 0.5

    # --- Milestone 2: Build Components ---
    banner "HW2 -- Build Components"
    send_msg "$PEER_PM" "Start milestone HW2 -- dispatch builders"
    skd tell "$PEER_PM" "Start milestone HW2 -- build components" || true
    send_signal "hw2-start"

    task_set HW2 status=in_progress
    task_set HW2.1 status=in_progress
    task_set HW2.2 status=in_progress

    say "Waiting for builder reports..."
    wait_signal "hw2-complete"

    task_set HW2.1 status=completed
    task_set HW2.2 status=completed
    task_set HW2 status=completed
    ok "HW2 complete -- both components built!"
    sleep 0.5

    # --- Milestone 3: Integration Check ---
    banner "HW3 -- Integration Check"
    send_msg "$PEER_PM" "Start milestone HW3 -- verify integration"
    skd tell "$PEER_PM" "Start milestone HW3 -- integration check" || true
    send_signal "hw3-start"

    task_set HW3 status=in_progress
    say "Waiting for checker verification..."
    wait_signal "hw3-complete"

    task_set HW3.1 status=completed
    task_set HW3.2 status=completed
    task_set HW3 status=completed
    ok "HW3 complete -- integration verified!"
    sleep 0.5

    # --- Milestone 4: Final Signoff ---
    banner "HW4 -- Final Signoff"
    send_msg "$PEER_PM" "Start milestone HW4 -- final signoff"
    skd tell "$PEER_PM" "Start milestone HW4 -- final signoff" || true
    task_set HW4 status=in_progress

    sleep 1
    task_set HW4.1 status=completed
    ok "HW4.1 -- Release notes written"
    sleep 0.5
    task_set HW4.2 status=completed
    ok "HW4.2 -- Stakeholder approval"
    task_set HW4 status=completed

    echo ""
    echo "${bold}${green}+=============================================+${reset}"
    echo "${bold}${green}|     ALL MILESTONES COMPLETE                 |${reset}"
    echo "${bold}${green}|     HW1 +  HW2 +  HW3 +  HW4 +            |${reset}"
    echo "${bold}${green}+=============================================+${reset}"
    echo ""
    ;;

  pm)
    say "PM agent ready -- monitoring for pilot directives..."

    # Wait for HW1 signal (acknowledge only)
    wait_signal "hw1-start"
    say "Acknowledged HW1 -- pilot handling setup"
    sleep 0.5

    # Wait for HW2 signal -- dispatch builders
    wait_signal "hw2-start"
    banner "Dispatching Builders"

    send_msg "$PEER_WORKER1" "Build component Alpha"
    skd agent assign "$PEER_WORKER1" HW2.1 2>/dev/null || true
    skd tell "$PEER_WORKER1" "Build component Alpha -- create alpha.built" || true
    sleep 0.3

    send_msg "$PEER_WORKER2" "Build component Beta"
    skd agent assign "$PEER_WORKER2" HW2.2 2>/dev/null || true
    skd tell "$PEER_WORKER2" "Build component Beta -- create beta.built" || true

    send_signal "dispatch-builders"

    say "Monitoring builder progress..."
    wait_signal "alpha-done"
    ok "Alpha build complete"
    wait_signal "beta-done"
    ok "Beta build complete"

    ok "Both builders finished!"
    skd tell "$PEER_PILOT" "Milestone HW2 complete -- both components built" || true
    send_signal "hw2-complete"
    sleep 0.5

    # Wait for HW3 signal -- dispatch checker
    wait_signal "hw3-start"
    banner "Dispatching Checker"

    send_msg "$PEER_CHECKER" "Verify Alpha + Beta integration"
    skd agent assign "$PEER_CHECKER" HW3.1 2>/dev/null || true
    skd tell "$PEER_CHECKER" "Verify Alpha and Beta integration" || true
    send_signal "dispatch-checker"

    say "Waiting for verification..."
    wait_signal "check-done"
    ok "Integration check passed!"
    skd tell "$PEER_PILOT" "Milestone HW3 complete -- integration verified" || true
    send_signal "hw3-complete"

    ok "PM work complete -- standing by."
    ;;

  builder)
    # Determine component from name
    if [[ "$NAME" == *worker1 ]]; then
      COMPONENT="alpha"
      TASK_ID="HW2.1"
      TASK_DIR="02_build_component_alpha"
      BUILD_DELAY=0.7
    else
      COMPONENT="beta"
      TASK_ID="HW2.2"
      TASK_DIR="03_build_component_beta"
      BUILD_DELAY=1.0
    fi

    say "Builder ready -- assigned to component ${bold}$COMPONENT${reset}"
    say "Waiting for dispatch..."
    wait_signal "dispatch-builders"

    say "Starting build of ${bold}$COMPONENT${reset}..."
    task_set "$TASK_ID" status=in_progress

    # Simulate build with progress bar
    for pct in 0 20 40 60 80 100; do
      progress_bar "$pct" "Building $COMPONENT..."
      sleep "$BUILD_DELAY"
    done

    # Create artifact
    mkdir -p "$HW_DIR/$TASK_DIR"
    cat > "$HW_DIR/$TASK_DIR/$COMPONENT.built" <<EOF
agent: $NAME
component: $COMPONENT
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
status: success
EOF

    ok "Build complete -- ${bold}$COMPONENT.built${reset} created"
    task_set "$TASK_ID" status=completed
    skd tell "$PEER_PM" "Task $TASK_ID done -- $COMPONENT built successfully" || true
    send_signal "${COMPONENT}-done"
    ok "$NAME finished. Standing by."
    ;;

  checker)
    say "Checker ready -- waiting for dispatch..."

    wait_signal "dispatch-checker"
    say "Starting integration verification..."

    ALPHA="$HW_DIR/02_build_component_alpha/alpha.built"
    BETA="$HW_DIR/03_build_component_beta/beta.built"

    sleep 0.5
    say "Checking alpha artifact..."
    if [[ -f "$ALPHA" ]]; then
      ok "alpha.built: $(head -1 "$ALPHA")"
    else
      say "alpha.built: ${bold}MISSING${reset}"
    fi

    sleep 0.5
    say "Checking beta artifact..."
    if [[ -f "$BETA" ]]; then
      ok "beta.built: $(head -1 "$BETA")"
    else
      say "beta.built: ${bold}MISSING${reset}"
    fi

    sleep 0.5
    say "Running integration tests..."
    for pct in 0 25 50 75 100; do
      progress_bar "$pct" "Verifying integration..."
      sleep 0.7
    done

    mkdir -p "$HW_DIR/04_integration_check"
    cat > "$HW_DIR/04_integration_check/integration.verified" <<EOF
checker: $NAME
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
alpha: $([ -f "$ALPHA" ] && echo "present" || echo "missing")
beta: $([ -f "$BETA" ] && echo "present" || echo "missing")
result: $([ -f "$ALPHA" ] && [ -f "$BETA" ] && echo "pass" || echo "fail")
EOF

    ok "Integration check complete -- PASS"
    skd tell "$PEER_PM" "Verification complete -- integration check passed" || true
    send_signal "check-done"
    ok "$NAME finished. Standing by."
    ;;
esac

# ── Phase 3: Completion ─────────────────────────────────────────
ok "$NAME ($ROLE) finished all work."
say "Final task state:"
skd task list 2>/dev/null || true
say "Standing by. (Ctrl-C to exit)"
# Keep alive so pane stays visible
while true; do sleep 60; done

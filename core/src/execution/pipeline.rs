//! Multi-step task pipelines — step sequencing, conditions, and result tracking.
//!
//! A `Pipeline` is an ordered sequence of `PipelineStep` items, each with
//! optional conditions and error-handling policy. The pipeline tracks per-step
//! results and provides methods for advancing through steps.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// StepCondition
// ---------------------------------------------------------------------------

/// Condition that determines whether a pipeline step should execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "condition", rename_all = "snake_case")]
pub enum StepCondition {
    Always,
    OnSuccess,
    OnFailure,
    ExitCodeEquals { code: i32 },
    ExitCodeNonZero,
}

impl StepCondition {
    /// Evaluate this condition against the previous step's exit code.
    /// Returns true if the step should execute.
    /// `prev_exit_code` is None if this is the first step.
    pub fn evaluate(&self, prev_exit_code: Option<i32>) -> bool {
        match self {
            StepCondition::Always => true,
            StepCondition::OnSuccess => prev_exit_code == Some(0),
            StepCondition::OnFailure => {
                matches!(prev_exit_code, Some(c) if c != 0)
            }
            StepCondition::ExitCodeEquals { code } => prev_exit_code == Some(*code),
            StepCondition::ExitCodeNonZero => {
                matches!(prev_exit_code, Some(c) if c != 0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PipelineStep
// ---------------------------------------------------------------------------

/// A single step in a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub name: String,
    pub command: Vec<String>,
    pub working_dir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub continue_on_error: bool,
    pub condition: Option<StepCondition>,
}

// ---------------------------------------------------------------------------
// StepStatus
// ---------------------------------------------------------------------------

/// The status of a single pipeline step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    TimedOut,
}

// ---------------------------------------------------------------------------
// StepResult
// ---------------------------------------------------------------------------

/// The result of executing a pipeline step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_name: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_lines: usize,
    pub status: StepStatus,
}

// ---------------------------------------------------------------------------
// PipelineStatus
// ---------------------------------------------------------------------------

/// Overall status of a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// An ordered sequence of steps with result tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub name: String,
    pub steps: Vec<PipelineStep>,
    pub results: Vec<StepResult>,
    pub status: PipelineStatus,
    current_index: usize,
    started_ms: Option<u64>,
}

impl Pipeline {
    /// Create a new empty pipeline.
    pub fn new(name: &str) -> Self {
        Pipeline {
            name: name.to_string(),
            steps: Vec::new(),
            results: Vec::new(),
            status: PipelineStatus::Pending,
            current_index: 0,
            started_ms: None,
        }
    }

    /// Add a step to the pipeline. Can only add steps before the pipeline starts.
    pub fn add_step(&mut self, step: PipelineStep) -> Result<(), String> {
        if self.status != PipelineStatus::Pending {
            return Err("cannot add steps to a running or finished pipeline".into());
        }
        self.steps.push(step);
        Ok(())
    }

    /// Start the pipeline. Transitions from Pending to Running.
    pub fn start(&mut self, now_ms: u64) -> Result<(), String> {
        if self.status != PipelineStatus::Pending {
            return Err(format!("pipeline is {:?}, expected Pending", self.status));
        }
        if self.steps.is_empty() {
            return Err("cannot start an empty pipeline".into());
        }
        self.status = PipelineStatus::Running;
        self.started_ms = Some(now_ms);
        self.current_index = 0;
        Ok(())
    }

    /// Complete the current step with the given exit code and timing.
    ///
    /// Automatically advances to the next step or finishes the pipeline.
    pub fn complete_step(
        &mut self,
        exit_code: i32,
        duration_ms: u64,
        output_lines: usize,
        now_ms: u64,
    ) -> Result<(), String> {
        if self.status != PipelineStatus::Running {
            return Err("pipeline is not running".into());
        }
        if self.current_index >= self.steps.len() {
            return Err("no more steps to complete".into());
        }

        let step = &self.steps[self.current_index];
        let step_name = step.name.clone();
        let continue_on_error = step.continue_on_error;

        let status = if exit_code == 0 {
            StepStatus::Succeeded
        } else {
            StepStatus::Failed
        };

        self.results.push(StepResult {
            step_name,
            exit_code: Some(exit_code),
            duration_ms,
            output_lines,
            status: status.clone(),
        });

        self.current_index += 1;

        // If step failed and we should not continue, fail the pipeline.
        if status == StepStatus::Failed && !continue_on_error {
            self.status = PipelineStatus::Failed;
            // Skip remaining steps.
            self.skip_remaining();
            return Ok(());
        }
        self.advance_skipping_conditions(now_ms);

        Ok(())
    }

    /// Skip the current step with a given reason (records as Skipped).
    pub fn skip_step(&mut self, reason: &str) -> Result<(), String> {
        if self.status != PipelineStatus::Running {
            return Err("pipeline is not running".into());
        }
        if self.current_index >= self.steps.len() {
            return Err("no more steps to skip".into());
        }

        let step_name = self.steps[self.current_index].name.clone();
        let _ = reason; // Reason noted but not stored in StepResult currently.

        self.results.push(StepResult {
            step_name,
            exit_code: None,
            duration_ms: 0,
            output_lines: 0,
            status: StepStatus::Skipped,
        });

        self.current_index += 1;

        if self.current_index >= self.steps.len() {
            self.status = PipelineStatus::Completed;
        }

        Ok(())
    }

    /// Get the current step, if any.
    pub fn current_step(&self) -> Option<&PipelineStep> {
        if self.status != PipelineStatus::Running {
            return None;
        }
        self.steps.get(self.current_index)
    }

    /// Whether the pipeline has finished (completed, failed, or cancelled).
    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            PipelineStatus::Completed | PipelineStatus::Failed | PipelineStatus::Cancelled
        )
    }

    /// Whether all completed steps succeeded.
    pub fn overall_success(&self) -> bool {
        self.status == PipelineStatus::Completed
            && self
                .results
                .iter()
                .all(|r| r.status == StepStatus::Succeeded || r.status == StepStatus::Skipped)
    }

    /// Generate a summary string of pipeline execution.
    pub fn summary(&self) -> String {
        let total = self.steps.len();
        let completed = self.results.len();
        let succeeded = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Succeeded)
            .count();
        let failed = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Failed)
            .count();
        let skipped = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Skipped)
            .count();

        format!(
            "{}: {:?} — {}/{} steps ({} ok, {} failed, {} skipped)",
            self.name, self.status, completed, total, succeeded, failed, skipped
        )
    }

    /// Cancel the pipeline.
    pub fn cancel(&mut self) -> Result<(), String> {
        if self.is_complete() {
            return Err("pipeline is already complete".into());
        }
        self.status = PipelineStatus::Cancelled;
        self.skip_remaining();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Skip all remaining steps (mark as Skipped in results).
    fn skip_remaining(&mut self) {
        while self.current_index < self.steps.len() {
            let step_name = self.steps[self.current_index].name.clone();
            self.results.push(StepResult {
                step_name,
                exit_code: None,
                duration_ms: 0,
                output_lines: 0,
                status: StepStatus::Skipped,
            });
            self.current_index += 1;
        }
    }

    /// Advance past steps whose conditions are not met (auto-skip).
    fn advance_skipping_conditions(&mut self, _now_ms: u64) {
        let prev_exit = self
            .results
            .last()
            .and_then(|r| r.exit_code);

        while self.current_index < self.steps.len() {
            let step = &self.steps[self.current_index];
            if let Some(ref condition) = step.condition {
                if !condition.evaluate(prev_exit) {
                    // Condition not met; auto-skip.
                    let step_name = step.name.clone();
                    self.results.push(StepResult {
                        step_name,
                        exit_code: None,
                        duration_ms: 0,
                        output_lines: 0,
                        status: StepStatus::Skipped,
                    });
                    self.current_index += 1;
                    continue;
                }
            }
            // Condition met or no condition — stop advancing.
            break;
        }

        // If we ran out of steps, mark pipeline as completed.
        if self.current_index >= self.steps.len() {
            self.status = PipelineStatus::Completed;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_step(name: &str) -> PipelineStep {
        PipelineStep {
            name: name.into(),
            command: vec!["echo".into(), name.into()],
            working_dir: None,
            timeout_ms: None,
            continue_on_error: false,
            condition: None,
        }
    }

    fn make_step_with_condition(name: &str, condition: StepCondition) -> PipelineStep {
        PipelineStep {
            name: name.into(),
            command: vec!["echo".into(), name.into()],
            working_dir: None,
            timeout_ms: None,
            continue_on_error: false,
            condition: Some(condition),
        }
    }

    fn make_step_continue_on_error(name: &str) -> PipelineStep {
        PipelineStep {
            name: name.into(),
            command: vec!["echo".into(), name.into()],
            working_dir: None,
            timeout_ms: None,
            continue_on_error: true,
            condition: None,
        }
    }

    #[test]
    fn new_pipeline() {
        let p = Pipeline::new("build");
        assert_eq!(p.name, "build");
        assert_eq!(p.status, PipelineStatus::Pending);
        assert!(p.steps.is_empty());
        assert!(p.results.is_empty());
    }

    #[test]
    fn add_steps() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();
        assert_eq!(p.steps.len(), 2);
    }

    #[test]
    fn add_step_after_start_fails() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.start(1000).unwrap();
        assert!(p.add_step(make_step("test")).is_err());
    }

    #[test]
    fn start_empty_pipeline_fails() {
        let mut p = Pipeline::new("build");
        assert!(p.start(1000).is_err());
    }

    #[test]
    fn start_twice_fails() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.start(1000).unwrap();
        assert!(p.start(2000).is_err());
    }

    #[test]
    fn basic_pipeline_flow() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();
        p.add_step(make_step("deploy")).unwrap();

        p.start(1000).unwrap();

        assert_eq!(p.current_step().unwrap().name, "compile");
        p.complete_step(0, 1000, 50, 2000).unwrap();

        assert_eq!(p.current_step().unwrap().name, "test");
        p.complete_step(0, 2000, 100, 4000).unwrap();

        assert_eq!(p.current_step().unwrap().name, "deploy");
        p.complete_step(0, 500, 10, 4500).unwrap();

        assert!(p.is_complete());
        assert!(p.overall_success());
        assert_eq!(p.status, PipelineStatus::Completed);
    }

    #[test]
    fn pipeline_failure_stops_execution() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();
        p.add_step(make_step("deploy")).unwrap();

        p.start(1000).unwrap();
        p.complete_step(1, 500, 10, 1500).unwrap(); // compile fails

        assert!(p.is_complete());
        assert_eq!(p.status, PipelineStatus::Failed);
        assert!(!p.overall_success());

        // test and deploy should be skipped.
        assert_eq!(p.results.len(), 3);
        assert_eq!(p.results[0].status, StepStatus::Failed);
        assert_eq!(p.results[1].status, StepStatus::Skipped);
        assert_eq!(p.results[2].status, StepStatus::Skipped);
    }

    #[test]
    fn continue_on_error() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step_continue_on_error("lint")).unwrap();
        p.add_step(make_step("test")).unwrap();

        p.start(1000).unwrap();
        p.complete_step(1, 500, 10, 1500).unwrap(); // lint fails but continues

        assert!(!p.is_complete());
        assert_eq!(p.current_step().unwrap().name, "test");

        p.complete_step(0, 1000, 50, 2500).unwrap();
        assert!(p.is_complete());
    }

    #[test]
    fn condition_on_success() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step("test")).unwrap();
        p.add_step(make_step_with_condition("deploy", StepCondition::OnSuccess))
            .unwrap();

        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap();

        // Condition met (exit 0), deploy should be current.
        assert_eq!(p.current_step().unwrap().name, "deploy");
    }

    #[test]
    fn condition_on_success_skips_on_failure() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step_continue_on_error("test")).unwrap();
        p.add_step(make_step_with_condition("deploy", StepCondition::OnSuccess))
            .unwrap();

        p.start(1000).unwrap();
        p.complete_step(1, 500, 10, 1500).unwrap(); // test fails

        // deploy condition not met (exit 1), auto-skipped, pipeline complete.
        assert!(p.is_complete());
        assert_eq!(p.results.len(), 2);
        assert_eq!(p.results[1].status, StepStatus::Skipped);
    }

    #[test]
    fn condition_on_failure() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step_continue_on_error("test")).unwrap();
        p.add_step(make_step_with_condition("notify", StepCondition::OnFailure))
            .unwrap();

        p.start(1000).unwrap();
        p.complete_step(1, 500, 10, 1500).unwrap(); // test fails

        // OnFailure condition met, notify should be current.
        assert_eq!(p.current_step().unwrap().name, "notify");
    }

    #[test]
    fn condition_on_failure_skips_on_success() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step("test")).unwrap();
        p.add_step(make_step_with_condition("rollback", StepCondition::OnFailure))
            .unwrap();

        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap(); // test succeeds

        // OnFailure not met, rollback skipped, pipeline complete.
        assert!(p.is_complete());
        assert_eq!(p.results[1].status, StepStatus::Skipped);
    }

    #[test]
    fn condition_always() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step_continue_on_error("test")).unwrap();
        p.add_step(make_step_with_condition("cleanup", StepCondition::Always))
            .unwrap();

        p.start(1000).unwrap();
        p.complete_step(1, 500, 10, 1500).unwrap();

        assert_eq!(p.current_step().unwrap().name, "cleanup");
    }

    #[test]
    fn condition_exit_code_equals() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step_continue_on_error("check")).unwrap();
        p.add_step(make_step_with_condition(
            "special",
            StepCondition::ExitCodeEquals { code: 42 },
        ))
        .unwrap();

        p.start(1000).unwrap();
        p.complete_step(42, 100, 5, 1100).unwrap();

        assert_eq!(p.current_step().unwrap().name, "special");
    }

    #[test]
    fn condition_exit_code_equals_mismatch() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step_continue_on_error("check")).unwrap();
        p.add_step(make_step_with_condition(
            "special",
            StepCondition::ExitCodeEquals { code: 42 },
        ))
        .unwrap();

        p.start(1000).unwrap();
        p.complete_step(0, 100, 5, 1100).unwrap();

        // Skipped because exit code was 0, not 42.
        assert!(p.is_complete());
        assert_eq!(p.results[1].status, StepStatus::Skipped);
    }

    #[test]
    fn skip_step() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();

        p.start(1000).unwrap();
        p.skip_step("not needed").unwrap();

        assert_eq!(p.current_step().unwrap().name, "test");
        assert_eq!(p.results[0].status, StepStatus::Skipped);
    }

    #[test]
    fn current_step_none_when_not_running() {
        let p = Pipeline::new("build");
        assert!(p.current_step().is_none());
    }

    #[test]
    fn summary_format() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();

        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap();
        p.complete_step(0, 500, 20, 2000).unwrap();

        let summary = p.summary();
        assert!(summary.contains("build"));
        assert!(summary.contains("2/2"));
        assert!(summary.contains("2 ok"));
    }

    #[test]
    fn cancel_running_pipeline() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();
        p.add_step(make_step("deploy")).unwrap();

        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap();
        p.cancel().unwrap();

        assert_eq!(p.status, PipelineStatus::Cancelled);
        assert!(p.is_complete());
        // test and deploy should be skipped.
        assert_eq!(p.results.len(), 3);
        assert_eq!(p.results[1].status, StepStatus::Skipped);
        assert_eq!(p.results[2].status, StepStatus::Skipped);
    }

    #[test]
    fn cancel_completed_fails() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap();
        assert!(p.cancel().is_err());
    }

    #[test]
    fn cancel_pending() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.cancel().unwrap();
        assert_eq!(p.status, PipelineStatus::Cancelled);
    }

    #[test]
    fn complete_step_when_not_running() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        assert!(p.complete_step(0, 500, 10, 1000).is_err());
    }

    #[test]
    fn pipeline_serde_round_trip() {
        let mut p = Pipeline::new("build");
        p.add_step(make_step("compile")).unwrap();
        p.add_step(make_step("test")).unwrap();
        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap();

        let json = serde_json::to_string(&p).unwrap();
        let back: Pipeline = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "build");
        assert_eq!(back.steps.len(), 2);
        assert_eq!(back.results.len(), 1);
    }

    #[test]
    fn step_condition_serde() {
        let conditions = vec![
            StepCondition::Always,
            StepCondition::OnSuccess,
            StepCondition::OnFailure,
            StepCondition::ExitCodeEquals { code: 42 },
            StepCondition::ExitCodeNonZero,
        ];

        for cond in &conditions {
            let json = serde_json::to_string(cond).unwrap();
            let back: StepCondition = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *cond);
        }
    }

    #[test]
    fn step_status_serde() {
        let statuses = vec![
            StepStatus::Pending,
            StepStatus::Running,
            StepStatus::Succeeded,
            StepStatus::Failed,
            StepStatus::Skipped,
            StepStatus::TimedOut,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let back: StepStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *status);
        }
    }

    #[test]
    fn pipeline_status_serde() {
        let statuses = vec![
            PipelineStatus::Pending,
            PipelineStatus::Running,
            PipelineStatus::Completed,
            PipelineStatus::Failed,
            PipelineStatus::Cancelled,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let back: PipelineStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *status);
        }
    }

    #[test]
    fn condition_evaluate_first_step() {
        // First step: prev_exit_code is None.
        assert!(StepCondition::Always.evaluate(None));
        assert!(!StepCondition::OnSuccess.evaluate(None));
        assert!(!StepCondition::OnFailure.evaluate(None));
    }

    #[test]
    fn condition_evaluate_success() {
        assert!(StepCondition::OnSuccess.evaluate(Some(0)));
        assert!(!StepCondition::OnFailure.evaluate(Some(0)));
        assert!(!StepCondition::ExitCodeNonZero.evaluate(Some(0)));
    }

    #[test]
    fn condition_evaluate_failure() {
        assert!(!StepCondition::OnSuccess.evaluate(Some(1)));
        assert!(StepCondition::OnFailure.evaluate(Some(1)));
        assert!(StepCondition::ExitCodeNonZero.evaluate(Some(1)));
    }

    #[test]
    fn pipeline_with_working_dir_and_timeout() {
        let step = PipelineStep {
            name: "test".into(),
            command: vec!["cargo".into(), "test".into()],
            working_dir: Some("/project".into()),
            timeout_ms: Some(60000),
            continue_on_error: false,
            condition: None,
        };

        let json = serde_json::to_string(&step).unwrap();
        let back: PipelineStep = serde_json::from_str(&json).unwrap();
        assert_eq!(back.working_dir, Some("/project".into()));
        assert_eq!(back.timeout_ms, Some(60000));
    }

    #[test]
    fn overall_success_with_skipped() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step("test")).unwrap();
        p.add_step(make_step_with_condition("rollback", StepCondition::OnFailure))
            .unwrap();

        p.start(1000).unwrap();
        p.complete_step(0, 500, 10, 1500).unwrap();

        // rollback is auto-skipped, pipeline complete.
        assert!(p.overall_success());
    }

    #[test]
    fn overall_success_false_on_failure() {
        let mut p = Pipeline::new("ci");
        p.add_step(make_step("test")).unwrap();

        p.start(1000).unwrap();
        p.complete_step(1, 500, 10, 1500).unwrap();

        assert!(!p.overall_success());
    }
}

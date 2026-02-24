use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskSource {
    Roadmap,
    Filesystem,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub title: String,
    pub source: TaskSource,
    pub status: TaskStatus,
    pub result: Option<String>,
    pub agent: Option<String>,
    pub children: Vec<TaskNode>,
    pub spec_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_node_round_trip() {
        let task = TaskNode {
            id: "CMX1".into(),
            title: "Core daemon".into(),
            source: TaskSource::Roadmap,
            status: TaskStatus::InProgress,
            result: None,
            agent: Some("worker-1".into()),
            children: vec![TaskNode {
                id: "CMX1A".into(),
                title: "Socket protocol".into(),
                source: TaskSource::Filesystem,
                status: TaskStatus::Pending,
                result: None,
                agent: None,
                children: vec![],
                spec_path: Some("/tasks/CMX1A/CMX1A.md".into()),
            }],
            spec_path: Some("/tasks/CMX1/CMX1.md".into()),
        };
        let json = serde_json::to_string(&task).unwrap();
        let back: TaskNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "CMX1");
        assert_eq!(back.children.len(), 1);
        assert_eq!(back.children[0].id, "CMX1A");
    }

    #[test]
    fn task_status_serde() {
        let json = serde_json::to_string(&TaskStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");
    }
}

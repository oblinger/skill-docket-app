use serde::{Deserialize, Serialize};

// Command is now defined in crate::command::Command.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { output: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    CreateSession { name: String, cwd: String },
    KillSession { name: String },
    SplitPane { session: String, direction: Direction, percent: u32 },
    PlaceAgent { pane_id: String, agent: String },
    CreateAgent { name: String, role: String, path: String },
    KillAgent { name: String },
    ConnectSsh { agent: String, host: String, port: u16 },
    UpdateAssignment { agent: String, task: Option<String> },
    SendKeys { target: String, keys: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_ok() {
        let resp = Response::Ok { output: "all good".into() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn response_error() {
        let resp = Response::Error { message: "not found".into() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
    }

    #[test]
    fn action_create_session() {
        let action = Action::CreateSession {
            name: "work".into(),
            cwd: "/tmp".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"create_session\""));
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }

    #[test]
    fn action_split_pane() {
        let action = Action::SplitPane {
            session: "work".into(),
            direction: Direction::Horizontal,
            percent: 50,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"direction\":\"horizontal\""));
    }

    #[test]
    fn action_create_agent() {
        let action = Action::CreateAgent {
            name: "worker-1".into(),
            role: "worker".into(),
            path: "/projects/cmx".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"role\":\"worker\""));
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxSession {
    pub name: String,
    pub windows: Vec<TmuxWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxWindow {
    pub index: u32,
    pub name: String,
    pub panes: Vec<TmuxPane>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxPane {
    pub id: String,
    pub index: u32,
    pub width: u32,
    pub height: u32,
    pub top: u32,
    pub left: u32,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutNode {
    Row { children: Vec<LayoutEntry> },
    Col { children: Vec<LayoutEntry> },
    Pane { agent: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutEntry {
    pub node: LayoutNode,
    pub percent: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip() {
        let session = TmuxSession {
            name: "cmx-main".into(),
            windows: vec![TmuxWindow {
                index: 0,
                name: "work".into(),
                panes: vec![TmuxPane {
                    id: "%0".into(),
                    index: 0,
                    width: 120,
                    height: 40,
                    top: 0,
                    left: 0,
                    agent: Some("worker-1".into()),
                }],
            }],
        };
        let json = serde_json::to_string(&session).unwrap();
        let back: TmuxSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "cmx-main");
        assert_eq!(back.windows[0].panes[0].id, "%0");
    }

    #[test]
    fn layout_node_tagged() {
        let layout = LayoutNode::Row {
            children: vec![
                LayoutEntry {
                    node: LayoutNode::Pane { agent: "pilot".into() },
                    percent: Some(30),
                },
                LayoutEntry {
                    node: LayoutNode::Pane { agent: "worker-1".into() },
                    percent: Some(70),
                },
            ],
        };
        let json = serde_json::to_string(&layout).unwrap();
        assert!(json.contains("\"type\":\"row\""));
        let back: LayoutNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, layout);
    }
}

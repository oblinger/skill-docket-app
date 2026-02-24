use serde::{Deserialize, Serialize};

use super::session::LayoutNode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TileKind {
    Agent,
    Composition,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tile {
    pub name: String,
    pub kind: TileKind,
    pub role: Option<String>,
    pub layout: Option<LayoutNode>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::session::LayoutEntry;

    #[test]
    fn tile_agent_round_trip() {
        let tile = Tile {
            name: "pilot".into(),
            kind: TileKind::Agent,
            role: Some("pilot".into()),
            layout: None,
        };
        let json = serde_json::to_string(&tile).unwrap();
        let back: Tile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tile);
    }

    #[test]
    fn tile_composition_with_layout() {
        let tile = Tile {
            name: "dev-env".into(),
            kind: TileKind::Composition,
            role: None,
            layout: Some(LayoutNode::Row {
                children: vec![
                    LayoutEntry {
                        node: LayoutNode::Pane { agent: "pilot".into() },
                        percent: Some(30),
                    },
                    LayoutEntry {
                        node: LayoutNode::Col {
                            children: vec![
                                LayoutEntry {
                                    node: LayoutNode::Pane { agent: "worker-1".into() },
                                    percent: Some(50),
                                },
                                LayoutEntry {
                                    node: LayoutNode::Pane { agent: "worker-2".into() },
                                    percent: Some(50),
                                },
                            ],
                        },
                        percent: Some(70),
                    },
                ],
            }),
        };
        let json = serde_json::to_string(&tile).unwrap();
        assert!(json.contains("\"kind\":\"composition\""));
        let back: Tile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tile);
    }

    #[test]
    fn tile_kind_serde() {
        let json = serde_json::to_string(&TileKind::Session).unwrap();
        assert_eq!(json, "\"session\"");
    }
}

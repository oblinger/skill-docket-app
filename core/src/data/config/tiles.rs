use crate::data::agent::AgentRegistry;
use crate::data::config::layout_expr::{parse_layout_expr, serialize_layout_expr};
use crate::types::session::{LayoutEntry, LayoutNode};
use crate::types::tiles::{Tile, TileKind};

#[derive(Debug, Clone)]
pub struct TileRegistry { pub tiles: Vec<Tile> }

impl TileRegistry {
    pub fn parse(content: &str) -> Result<Self, String> {
        let mut tiles = Vec::new();
        let mut cur_name: Option<String> = None;
        let mut cur_kind: Option<TileKind> = None;
        let mut cur_role: Option<String> = None;
        let mut cur_layout: Option<String> = None;
        for line in content.lines() {
            let t = line.trim();
            if let Some(heading) = t.strip_prefix("## ") {
                if let Some(name) = cur_name.take() {
                    let kind = cur_kind.take().unwrap_or(TileKind::Agent);
                    let layout = if let Some(e) = cur_layout.take() { Some(parse_layout_expr(&e)?) } else { None };
                    tiles.push(Tile { name, kind, role: cur_role.take(), layout });
                }
                cur_name = Some(heading.trim().to_string());
                cur_kind = None; cur_role = None; cur_layout = None;
            } else if let Some(v) = t.strip_prefix("kind:") {
                cur_kind = Some(match v.trim().to_lowercase().as_str() {
                    "agent" => TileKind::Agent, "composition" => TileKind::Composition,
                    "session" => TileKind::Session, other => return Err(format!("unknown kind: {}", other)),
                });
            } else if let Some(v) = t.strip_prefix("role:") { cur_role = Some(v.trim().to_string()); }
              else if let Some(v) = t.strip_prefix("layout:") { cur_layout = Some(v.trim().to_string()); }
        }
        if let Some(name) = cur_name.take() {
            let kind = cur_kind.take().unwrap_or(TileKind::Agent);
            let layout = if let Some(e) = cur_layout.take() { Some(parse_layout_expr(&e)?) } else { None };
            tiles.push(Tile { name, kind, role: cur_role.take(), layout });
        }
        Ok(TileRegistry { tiles })
    }
    pub fn get(&self, name: &str) -> Option<&Tile> { self.tiles.iter().find(|t| t.name == name) }
    pub fn instantiate(&self, tile_name: &str, agent_registry: &AgentRegistry) -> Result<LayoutNode, String> {
        let tile = self.get(tile_name).ok_or_else(|| format!("tile not found: {}", tile_name))?;
        let layout = tile.layout.as_ref().ok_or_else(|| format!("tile '{}' has no layout", tile_name))?;
        let mut name_map: Vec<(String, String)> = Vec::new();
        collect_pane_agents(layout, &mut name_map, agent_registry);
        Ok(rename_panes(layout, &name_map))
    }
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for tile in &self.tiles {
            out.push_str(&format!("## {}\n", tile.name));
            out.push_str(&format!("kind: {}\n", match tile.kind { TileKind::Agent => "agent", TileKind::Composition => "composition", TileKind::Session => "session" }));
            if let Some(ref role) = tile.role { out.push_str(&format!("role: {}\n", role)); }
            if let Some(ref layout) = tile.layout { out.push_str(&format!("layout: {}\n", serialize_layout_expr(layout))); }
            out.push('\n');
        }
        out
    }
}

fn collect_pane_agents(node: &LayoutNode, name_map: &mut Vec<(String, String)>, ar: &AgentRegistry) {
    match node {
        LayoutNode::Pane { agent } => {
            let existing = name_map.iter().filter(|(o, _)| o == agent).count();
            let base = ar.next_name(agent);
            let num_str: String = base.chars().skip_while(|c| !c.is_ascii_digit()).collect();
            let base_num: u32 = num_str.parse().unwrap_or(1);
            name_map.push((agent.clone(), format!("{}{}", agent.to_lowercase(), base_num + existing as u32)));
        }
        LayoutNode::Row { children } | LayoutNode::Col { children } => {
            for e in children { collect_pane_agents(&e.node, name_map, ar); }
        }
    }
}

fn rename_panes(node: &LayoutNode, map: &[(String, String)]) -> LayoutNode {
    let mut idx = 0; rename_inner(node, map, &mut idx)
}
fn rename_inner(node: &LayoutNode, map: &[(String, String)], idx: &mut usize) -> LayoutNode {
    match node {
        LayoutNode::Pane { .. } => {
            if *idx < map.len() { let n = map[*idx].1.clone(); *idx += 1; LayoutNode::Pane { agent: n } }
            else { node.clone() }
        }
        LayoutNode::Row { children } => LayoutNode::Row {
            children: children.iter().map(|e| LayoutEntry { node: rename_inner(&e.node, map, idx), percent: e.percent }).collect()
        },
        LayoutNode::Col { children } => LayoutNode::Col {
            children: children.iter().map(|e| LayoutEntry { node: rename_inner(&e.node, map, idx), percent: e.percent }).collect()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::agent::{Agent, AgentStatus, AgentType, HealthState};
    fn make_agent(name: &str, role: &str) -> Agent {
        Agent { name: name.into(), role: role.into(), agent_type: AgentType::Claude, task: None, path: "/tmp".into(), status: AgentStatus::Idle, status_notes: String::new(), health: HealthState::Unknown, last_heartbeat_ms: None, session: None }
    }
    #[test] fn parse_tile_with_layout() {
        let r = TileRegistry::parse("## two-workers\nkind: composition\nlayout: ROW(worker 50%, worker 50%)\n").unwrap();
        assert_eq!(r.tiles.len(), 1); assert!(r.tiles[0].layout.is_some());
    }
    #[test] fn parse_tile_agent_with_role() {
        let r = TileRegistry::parse("## solo\nkind: agent\nrole: pilot\n").unwrap();
        assert_eq!(r.tiles[0].role.as_deref(), Some("pilot"));
    }
    #[test] fn parse_multiple_tiles() {
        let r = TileRegistry::parse("## a\nkind: agent\n\n## b\nkind: session\n").unwrap();
        assert_eq!(r.tiles.len(), 2);
    }
    #[test] fn instantiate_with_unique_names() {
        let mut ar = AgentRegistry::new();
        ar.add(make_agent("worker1", "worker")).unwrap();
        ar.add(make_agent("worker2", "worker")).unwrap();
        let tr = TileRegistry::parse("## dp\nkind: composition\nlayout: ROW(worker 50%, worker 50%)\n").unwrap();
        let result = tr.instantiate("dp", &ar).unwrap();
        match result { LayoutNode::Row { children } => {
            match &children[0].node { LayoutNode::Pane { agent } => assert_eq!(agent, "worker3"), _ => panic!() }
            match &children[1].node { LayoutNode::Pane { agent } => assert_eq!(agent, "worker4"), _ => panic!() }
        } _ => panic!() }
    }
    #[test] fn instantiate_not_found() { assert!(TileRegistry::parse("").unwrap().instantiate("x", &AgentRegistry::new()).is_err()); }
    #[test] fn instantiate_no_layout() {
        let tr = TileRegistry::parse("## solo\nkind: agent\nrole: pilot\n").unwrap();
        assert!(tr.instantiate("solo", &AgentRegistry::new()).is_err());
    }
    #[test] fn round_trip_tiles() {
        let r = TileRegistry::parse("## solo\nkind: agent\nrole: pilot\n").unwrap();
        let r2 = TileRegistry::parse(&r.serialize()).unwrap();
        assert_eq!(r.tiles.len(), r2.tiles.len());
    }
    #[test] fn round_trip_tiles_with_layout() {
        let r = TileRegistry::parse("## p\nkind: composition\nlayout: ROW(a 50%, b 50%)\n").unwrap();
        let r2 = TileRegistry::parse(&r.serialize()).unwrap();
        assert_eq!(r.tiles[0].layout, r2.tiles[0].layout);
    }
    #[test] fn get_tile_by_name() {
        let r = TileRegistry::parse("## a\nkind: agent\n\n## b\nkind: session\n").unwrap();
        assert!(r.get("a").is_some()); assert!(r.get("c").is_none());
    }
}

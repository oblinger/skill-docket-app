use crate::types::agent::{Agent, AgentStatus, HealthState};


/// In-memory registry of all agents known to the system.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    agents: Vec<Agent>,
}


impl AgentRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        AgentRegistry { agents: Vec::new() }
    }

    /// Add an agent. Fails if an agent with the same name already exists.
    pub fn add(&mut self, agent: Agent) -> Result<(), String> {
        if self.agents.iter().any(|a| a.name == agent.name) {
            return Err(format!("agent already exists: {}", agent.name));
        }
        self.agents.push(agent);
        Ok(())
    }

    /// Remove an agent by name, returning it. Fails if not found.
    pub fn remove(&mut self, name: &str) -> Result<Agent, String> {
        let pos = self
            .agents
            .iter()
            .position(|a| a.name == name)
            .ok_or_else(|| format!("agent not found: {}", name))?;
        Ok(self.agents.remove(pos))
    }

    /// Look up an agent by name.
    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// Mutable look-up by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.name == name)
    }

    /// Return a slice of all agents.
    pub fn list(&self) -> &[Agent] {
        &self.agents
    }

    /// Find all agents whose role matches (case-insensitive).
    pub fn find_by_role(&self, role: &str) -> Vec<&Agent> {
        let role_lower = role.to_lowercase();
        self.agents
            .iter()
            .filter(|a| a.role.to_lowercase() == role_lower)
            .collect()
    }

    /// Generate the next sequential name for a given role.
    /// E.g., if "worker1" and "worker2" exist, returns "worker3".
    pub fn next_name(&self, role: &str) -> String {
        let role_lower = role.to_lowercase();
        let mut max_num: u32 = 0;
        for a in &self.agents {
            if a.role.to_lowercase() == role_lower {
                // Try to extract a trailing number from the name
                let suffix = a.name.trim_start_matches(|c: char| !c.is_ascii_digit());
                if let Ok(n) = suffix.parse::<u32>() {
                    if n >= max_num {
                        max_num = n + 1;
                    }
                } else {
                    // Agent exists with no number; next starts at 1
                    if max_num == 0 {
                        max_num = 1;
                    }
                }
            }
        }
        if max_num == 0 {
            max_num = 1;
        }
        format!("{}{}", role_lower, max_num)
    }

    /// Assign an agent to a task. Sets `agent.task` and status to Busy.
    pub fn assign(&mut self, agent_name: &str, task: &str) -> Result<(), String> {
        let agent = self
            .get_mut(agent_name)
            .ok_or_else(|| format!("agent not found: {}", agent_name))?;
        agent.task = Some(task.to_string());
        agent.status = AgentStatus::Busy;
        Ok(())
    }

    /// Unassign an agent from its current task. Returns the old task name if any.
    /// Sets status to Idle.
    pub fn unassign(&mut self, agent_name: &str) -> Result<Option<String>, String> {
        let agent = self
            .get_mut(agent_name)
            .ok_or_else(|| format!("agent not found: {}", agent_name))?;
        let old = agent.task.take();
        agent.status = AgentStatus::Idle;
        Ok(old)
    }

    /// Update the status_notes field for the named agent.
    pub fn update_status(&mut self, agent_name: &str, notes: &str) -> Result<(), String> {
        let agent = self
            .get_mut(agent_name)
            .ok_or_else(|| format!("agent not found: {}", agent_name))?;
        agent.status_notes = notes.to_string();
        Ok(())
    }

    /// Update the health state for the named agent.
    pub fn update_health(
        &mut self,
        agent_name: &str,
        health: HealthState,
    ) -> Result<(), String> {
        let agent = self
            .get_mut(agent_name)
            .ok_or_else(|| format!("agent not found: {}", agent_name))?;
        agent.health = health;
        Ok(())
    }
}


impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::agent::AgentType;

    fn make_agent(name: &str, role: &str) -> Agent {
        Agent {
            name: name.into(),
            role: role.into(),
            agent_type: AgentType::Claude,
            task: None,
            path: "/tmp".into(),
            status: AgentStatus::Idle,
            status_notes: String::new(),
            health: HealthState::Unknown,
            last_heartbeat_ms: None,
            session: None,
        }
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = AgentRegistry::new();
        assert!(reg.list().is_empty());
    }

    #[test]
    fn add_and_get() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        assert!(reg.get("w1").is_some());
        assert_eq!(reg.get("w1").unwrap().role, "worker");
    }

    #[test]
    fn add_duplicate_fails() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        let result = reg.add(make_agent("w1", "pilot"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn remove_existing() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        let removed = reg.remove("w1").unwrap();
        assert_eq!(removed.name, "w1");
        assert!(reg.get("w1").is_none());
    }

    #[test]
    fn remove_missing_fails() {
        let mut reg = AgentRegistry::new();
        let result = reg.remove("nope");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn get_mut_modifies() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        reg.get_mut("w1").unwrap().status = AgentStatus::Busy;
        assert_eq!(reg.get("w1").unwrap().status, AgentStatus::Busy);
    }

    #[test]
    fn find_by_role() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        reg.add(make_agent("w2", "worker")).unwrap();
        reg.add(make_agent("p1", "pilot")).unwrap();
        let workers = reg.find_by_role("worker");
        assert_eq!(workers.len(), 2);
        let pilots = reg.find_by_role("pilot");
        assert_eq!(pilots.len(), 1);
    }

    #[test]
    fn find_by_role_case_insensitive() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "Worker")).unwrap();
        let found = reg.find_by_role("worker");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn next_name_empty() {
        let reg = AgentRegistry::new();
        assert_eq!(reg.next_name("worker"), "worker1");
    }

    #[test]
    fn next_name_sequential() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_agent("worker2", "worker")).unwrap();
        assert_eq!(reg.next_name("worker"), "worker3");
    }

    #[test]
    fn next_name_with_gap() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_agent("worker5", "worker")).unwrap();
        // Should pick one past the max
        assert_eq!(reg.next_name("worker"), "worker6");
    }

    #[test]
    fn assign_and_unassign() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        reg.assign("w1", "CMX1").unwrap();
        assert_eq!(reg.get("w1").unwrap().task.as_deref(), Some("CMX1"));
        assert_eq!(reg.get("w1").unwrap().status, AgentStatus::Busy);

        let old = reg.unassign("w1").unwrap();
        assert_eq!(old, Some("CMX1".into()));
        assert_eq!(reg.get("w1").unwrap().task, None);
        assert_eq!(reg.get("w1").unwrap().status, AgentStatus::Idle);
    }

    #[test]
    fn assign_missing_agent_fails() {
        let mut reg = AgentRegistry::new();
        let result = reg.assign("nobody", "CMX1");
        assert!(result.is_err());
    }

    #[test]
    fn unassign_missing_agent_fails() {
        let mut reg = AgentRegistry::new();
        let result = reg.unassign("nobody");
        assert!(result.is_err());
    }

    #[test]
    fn unassign_already_idle() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        let old = reg.unassign("w1").unwrap();
        assert_eq!(old, None);
    }

    #[test]
    fn update_status_notes() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        reg.update_status("w1", "compiling...").unwrap();
        assert_eq!(reg.get("w1").unwrap().status_notes, "compiling...");
    }

    #[test]
    fn update_health() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("w1", "worker")).unwrap();
        reg.update_health("w1", HealthState::Degraded).unwrap();
        assert_eq!(reg.get("w1").unwrap().health, HealthState::Degraded);
    }

    #[test]
    fn update_health_missing_agent() {
        let mut reg = AgentRegistry::new();
        let result = reg.update_health("nobody", HealthState::Healthy);
        assert!(result.is_err());
    }

    #[test]
    fn list_preserves_order() {
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("alpha", "worker")).unwrap();
        reg.add(make_agent("beta", "worker")).unwrap();
        reg.add(make_agent("gamma", "pilot")).unwrap();
        let names: Vec<&str> = reg.list().iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }
}

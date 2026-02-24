use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::data::agent::AgentRegistry;
use crate::types::agent::AgentStatus;


/// Configuration for a role's worker pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoolConfig {
    pub target_size: u32,
    pub auto_expand: bool,
    pub max_size: u32,
    pub path: String,
}


/// Per-role pool state.
#[derive(Debug, Clone)]
pub struct PoolState {
    pub role: String,
    pub config: PoolConfig,
    pub idle_count: u32,
    pub busy_count: u32,
    pub spawning_count: u32,
    pub total: u32,
}


/// Manages worker pools across all configured roles.
pub struct PoolManager {
    configs: HashMap<String, PoolConfig>,
}


impl PoolManager {
    /// Create a new empty PoolManager.
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    /// Configure a pool for a role.
    pub fn set_pool(&mut self, role: &str, config: PoolConfig) {
        self.configs.insert(role.to_string(), config);
    }

    /// Remove a pool configuration.
    pub fn remove_pool(&mut self, role: &str) -> bool {
        self.configs.remove(role).is_some()
    }

    /// Get pool configuration for a role.
    pub fn get_config(&self, role: &str) -> Option<&PoolConfig> {
        self.configs.get(role)
    }

    /// List all configured pools.
    pub fn list_configs(&self) -> Vec<(&str, &PoolConfig)> {
        self.configs.iter().map(|(k, v)| (k.as_str(), v)).collect()
    }

    /// Compute current pool state by examining the agent registry.
    pub fn pool_state(&self, role: &str, registry: &AgentRegistry) -> Option<PoolState> {
        let config = self.configs.get(role)?;
        let agents = registry.find_by_role(role);
        let idle_count = agents
            .iter()
            .filter(|a| a.status == AgentStatus::Idle && a.task.is_none())
            .count() as u32;
        let busy_count = agents.iter().filter(|a| a.task.is_some()).count() as u32;
        let total = agents.len() as u32;
        Some(PoolState {
            role: role.to_string(),
            config: config.clone(),
            idle_count,
            busy_count,
            spawning_count: total.saturating_sub(idle_count + busy_count),
            total,
        })
    }

    /// Determine how many agents need to be spawned to reach target for a role.
    /// Returns 0 if pool is already at or above target.
    pub fn deficit(&self, role: &str, registry: &AgentRegistry) -> u32 {
        let state = match self.pool_state(role, registry) {
            Some(s) => s,
            None => return 0,
        };
        state.config.target_size.saturating_sub(state.total)
    }

    /// Determine deficits across all configured pools.
    pub fn all_deficits(&self, registry: &AgentRegistry) -> Vec<(String, u32)> {
        self.configs
            .keys()
            .filter_map(|role| {
                let d = self.deficit(role, registry);
                if d > 0 {
                    Some((role.clone(), d))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Generate spawn requests to fill all pool deficits.
    pub fn replenishment_names(
        &self,
        registry: &AgentRegistry,
    ) -> Vec<(String, String, String)> {
        let mut names = Vec::new();
        for (role, deficit) in self.all_deficits(registry) {
            let config = &self.configs[&role];
            for _ in 0..deficit {
                let name = registry.next_name(&role);
                names.push((name, role.clone(), config.path.clone()));
            }
        }
        names
    }

    /// Pick an idle worker from the pool for a given role.
    /// Returns the agent name if one is available, None if all busy.
    pub fn pick_idle(&self, role: &str, registry: &AgentRegistry) -> Option<String> {
        let agents = registry.find_by_role(role);
        agents
            .into_iter()
            .find(|a| a.task.is_none() && a.status == AgentStatus::Idle)
            .map(|a| a.name.clone())
    }

    /// Check if auto-expand should create a new worker.
    /// Returns true if the role's pool is configured for auto-expand,
    /// all members are busy, and total < max_size.
    pub fn should_auto_expand(&self, role: &str, registry: &AgentRegistry) -> bool {
        let state = match self.pool_state(role, registry) {
            Some(s) => s,
            None => return false,
        };
        state.config.auto_expand && state.idle_count == 0 && state.total < state.config.max_size
    }
}


impl Default for PoolManager {
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::agent::{Agent, AgentType, HealthState};

    fn make_pool_config(target: u32, path: &str) -> PoolConfig {
        PoolConfig {
            target_size: target,
            auto_expand: false,
            max_size: target * 2,
            path: path.to_string(),
        }
    }

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

    fn make_busy_agent(name: &str, role: &str, task: &str) -> Agent {
        Agent {
            name: name.into(),
            role: role.into(),
            agent_type: AgentType::Claude,
            task: Some(task.into()),
            path: "/tmp".into(),
            status: AgentStatus::Busy,
            status_notes: String::new(),
            health: HealthState::Unknown,
            last_heartbeat_ms: None,
            session: None,
        }
    }

    // 1. New PoolManager has no configs
    #[test]
    fn new_pool_manager_has_no_configs() {
        let pm = PoolManager::new();
        assert!(pm.list_configs().is_empty());
    }

    // 2. Set and get pool config
    #[test]
    fn set_and_get_pool_config() {
        let mut pm = PoolManager::new();
        let cfg = make_pool_config(3, "/tmp/work");
        pm.set_pool("worker", cfg.clone());
        let retrieved = pm.get_config("worker").unwrap();
        assert_eq!(retrieved, &cfg);
    }

    // 3. Remove pool config
    #[test]
    fn remove_pool_config() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        assert!(pm.remove_pool("worker"));
        assert!(pm.get_config("worker").is_none());
    }

    // 3b. Remove nonexistent pool returns false
    #[test]
    fn remove_nonexistent_pool() {
        let mut pm = PoolManager::new();
        assert!(!pm.remove_pool("ghost"));
    }

    // 4. List configs returns all
    #[test]
    fn list_configs_returns_all() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp/w"));
        pm.set_pool("pilot", make_pool_config(1, "/tmp/p"));
        let mut configs = pm.list_configs();
        configs.sort_by_key(|(k, _)| k.to_string());
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].0, "pilot");
        assert_eq!(configs[1].0, "worker");
    }

    // 5. Pool state: empty registry -> zeros
    #[test]
    fn pool_state_empty_registry_zeros() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let reg = AgentRegistry::new();
        let state = pm.pool_state("worker", &reg).unwrap();
        assert_eq!(state.idle_count, 0);
        assert_eq!(state.busy_count, 0);
        assert_eq!(state.spawning_count, 0);
        assert_eq!(state.total, 0);
    }

    // 6. Pool state: 3 idle workers -> idle_count=3
    #[test]
    fn pool_state_three_idle_workers() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_agent("worker2", "worker")).unwrap();
        reg.add(make_agent("worker3", "worker")).unwrap();
        let state = pm.pool_state("worker", &reg).unwrap();
        assert_eq!(state.idle_count, 3);
        assert_eq!(state.busy_count, 0);
        assert_eq!(state.total, 3);
    }

    // 7. Pool state: 2 busy + 1 idle -> correct counts
    #[test]
    fn pool_state_mixed_busy_idle() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T1")).unwrap();
        reg.add(make_busy_agent("worker3", "worker", "T2")).unwrap();
        let state = pm.pool_state("worker", &reg).unwrap();
        assert_eq!(state.idle_count, 1);
        assert_eq!(state.busy_count, 2);
        assert_eq!(state.total, 3);
    }

    // 8. Deficit: target 3, have 1 -> deficit 2
    #[test]
    fn deficit_target_3_have_1() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        assert_eq!(pm.deficit("worker", &reg), 2);
    }

    // 9. Deficit: target 3, have 3 -> deficit 0
    #[test]
    fn deficit_target_3_have_3() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_agent("worker2", "worker")).unwrap();
        reg.add(make_agent("worker3", "worker")).unwrap();
        assert_eq!(pm.deficit("worker", &reg), 0);
    }

    // 10. Deficit: target 3, have 5 -> deficit 0 (no kill)
    #[test]
    fn deficit_target_3_have_5() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let mut reg = AgentRegistry::new();
        for i in 1..=5 {
            reg.add(make_agent(&format!("worker{}", i), "worker"))
                .unwrap();
        }
        assert_eq!(pm.deficit("worker", &reg), 0);
    }

    // 11. All deficits across multiple roles
    #[test]
    fn all_deficits_multiple_roles() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp/w"));
        pm.set_pool("pilot", make_pool_config(2, "/tmp/p"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        // pilot has 0, needs 2
        let mut deficits = pm.all_deficits(&reg);
        deficits.sort_by_key(|(k, _)| k.clone());
        assert_eq!(deficits.len(), 2);
        // pilot needs 2, worker needs 2
        let pilot_def = deficits.iter().find(|(r, _)| r == "pilot").unwrap();
        let worker_def = deficits.iter().find(|(r, _)| r == "worker").unwrap();
        assert_eq!(pilot_def.1, 2);
        assert_eq!(worker_def.1, 2);
    }

    // 12. Replenishment names: generates correct entries
    #[test]
    fn replenishment_names_correct() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp/work"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        let names = pm.replenishment_names(&reg);
        assert_eq!(names.len(), 2);
        // Names should be worker2 and worker3 (generated by next_name)
        assert_eq!(names[0].1, "worker");
        assert_eq!(names[0].2, "/tmp/work");
    }

    // 13. Pick idle: returns idle worker
    #[test]
    fn pick_idle_returns_idle_worker() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T1")).unwrap();
        let picked = pm.pick_idle("worker", &reg);
        assert_eq!(picked, Some("worker1".into()));
    }

    // 14. Pick idle: all busy -> None
    #[test]
    fn pick_idle_all_busy_returns_none() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(2, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_busy_agent("worker1", "worker", "T1")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T2")).unwrap();
        let picked = pm.pick_idle("worker", &reg);
        assert!(picked.is_none());
    }

    // 15. Auto-expand: all busy + auto_expand=true + under max -> true
    #[test]
    fn auto_expand_all_busy_under_max() {
        let mut pm = PoolManager::new();
        pm.set_pool(
            "worker",
            PoolConfig {
                target_size: 2,
                auto_expand: true,
                max_size: 4,
                path: "/tmp".into(),
            },
        );
        let mut reg = AgentRegistry::new();
        reg.add(make_busy_agent("worker1", "worker", "T1")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T2")).unwrap();
        assert!(pm.should_auto_expand("worker", &reg));
    }

    // 16. Auto-expand: has idle -> false
    #[test]
    fn auto_expand_has_idle_returns_false() {
        let mut pm = PoolManager::new();
        pm.set_pool(
            "worker",
            PoolConfig {
                target_size: 2,
                auto_expand: true,
                max_size: 4,
                path: "/tmp".into(),
            },
        );
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T1")).unwrap();
        assert!(!pm.should_auto_expand("worker", &reg));
    }

    // 17. Auto-expand: at max_size -> false
    #[test]
    fn auto_expand_at_max_size_returns_false() {
        let mut pm = PoolManager::new();
        pm.set_pool(
            "worker",
            PoolConfig {
                target_size: 2,
                auto_expand: true,
                max_size: 2,
                path: "/tmp".into(),
            },
        );
        let mut reg = AgentRegistry::new();
        reg.add(make_busy_agent("worker1", "worker", "T1")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T2")).unwrap();
        assert!(!pm.should_auto_expand("worker", &reg));
    }

    // 18. Pool state for unconfigured role returns None
    #[test]
    fn pool_state_unconfigured_role() {
        let pm = PoolManager::new();
        let reg = AgentRegistry::new();
        assert!(pm.pool_state("ghost", &reg).is_none());
    }

    // 19. Deficit for unconfigured role returns 0
    #[test]
    fn deficit_unconfigured_role() {
        let pm = PoolManager::new();
        let reg = AgentRegistry::new();
        assert_eq!(pm.deficit("ghost", &reg), 0);
    }

    // 20. Pick idle for unconfigured role returns None
    #[test]
    fn pick_idle_unconfigured_role() {
        let pm = PoolManager::new();
        let reg = AgentRegistry::new();
        assert!(pm.pick_idle("ghost", &reg).is_none());
    }

    // 21. Auto-expand disabled -> false even when all busy
    #[test]
    fn auto_expand_disabled() {
        let mut pm = PoolManager::new();
        pm.set_pool(
            "worker",
            PoolConfig {
                target_size: 2,
                auto_expand: false,
                max_size: 4,
                path: "/tmp".into(),
            },
        );
        let mut reg = AgentRegistry::new();
        reg.add(make_busy_agent("worker1", "worker", "T1")).unwrap();
        reg.add(make_busy_agent("worker2", "worker", "T2")).unwrap();
        assert!(!pm.should_auto_expand("worker", &reg));
    }

    // 22. Default trait works
    #[test]
    fn default_pool_manager() {
        let pm = PoolManager::default();
        assert!(pm.list_configs().is_empty());
    }

    // 23. PoolConfig serde round-trip
    #[test]
    fn pool_config_serde_round_trip() {
        let cfg = make_pool_config(5, "/projects/work");
        let json = serde_json::to_string(&cfg).unwrap();
        let back: PoolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
    }

    // 24. Set pool overwrites existing config
    #[test]
    fn set_pool_overwrites() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(3, "/tmp/old"));
        pm.set_pool("worker", make_pool_config(5, "/tmp/new"));
        let cfg = pm.get_config("worker").unwrap();
        assert_eq!(cfg.target_size, 5);
        assert_eq!(cfg.path, "/tmp/new");
    }

    // 25. All deficits empty when all pools satisfied
    #[test]
    fn all_deficits_empty_when_satisfied() {
        let mut pm = PoolManager::new();
        pm.set_pool("worker", make_pool_config(2, "/tmp"));
        let mut reg = AgentRegistry::new();
        reg.add(make_agent("worker1", "worker")).unwrap();
        reg.add(make_agent("worker2", "worker")).unwrap();
        let deficits = pm.all_deficits(&reg);
        assert!(deficits.is_empty());
    }
}

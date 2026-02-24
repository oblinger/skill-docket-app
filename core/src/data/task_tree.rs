use crate::types::task::{TaskNode, TaskStatus};


/// The task tree holds all root-level tasks, each of which may have nested
/// children forming an arbitrary tree.
#[derive(Debug, Clone)]
pub struct TaskTree {
    roots: Vec<TaskNode>,
}


impl TaskTree {
    /// Create an empty task tree.
    pub fn new() -> Self {
        TaskTree { roots: Vec::new() }
    }

    /// Add a root-level task node.
    pub fn add_root(&mut self, node: TaskNode) {
        self.roots.push(node);
    }

    /// Recursively search for a task by id and return a reference.
    pub fn get(&self, id: &str) -> Option<&TaskNode> {
        for root in &self.roots {
            if let Some(found) = find_node(root, id) {
                return Some(found);
            }
        }
        None
    }

    /// Recursively search for a task by id and return a mutable reference.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        for root in &mut self.roots {
            if let Some(found) = find_node_mut(root, id) {
                return Some(found);
            }
        }
        None
    }

    /// Return a slice of all root-level tasks.
    pub fn roots(&self) -> &[TaskNode] {
        &self.roots
    }

    /// Set the status of a task by id. Fails if not found.
    pub fn set_status(&mut self, id: &str, status: TaskStatus) -> Result<(), String> {
        let node = self
            .get_mut(id)
            .ok_or_else(|| format!("task not found: {}", id))?;
        node.status = status;
        Ok(())
    }

    /// Assign an agent to a task. Sets status to InProgress.
    pub fn assign(&mut self, task_id: &str, agent: &str) -> Result<(), String> {
        let node = self
            .get_mut(task_id)
            .ok_or_else(|| format!("task not found: {}", task_id))?;
        node.agent = Some(agent.to_string());
        node.status = TaskStatus::InProgress;
        Ok(())
    }

    /// Unassign the agent from a task. Returns the old agent name if any.
    pub fn unassign(&mut self, task_id: &str) -> Result<Option<String>, String> {
        let node = self
            .get_mut(task_id)
            .ok_or_else(|| format!("task not found: {}", task_id))?;
        let old = node.agent.take();
        Ok(old)
    }

    /// Bottom-up status propagation: if all children of a node are Completed,
    /// the parent becomes Completed. If any child is InProgress, the parent
    /// becomes InProgress. If any child is Failed, the parent becomes Failed.
    pub fn propagate_status(&mut self) {
        for root in &mut self.roots {
            propagate_node(root);
        }
    }

    /// Depth-first flattened list of all tasks with their indent level.
    /// Level 0 = root tasks.
    pub fn flat_list(&self) -> Vec<(&TaskNode, usize)> {
        let mut result = Vec::new();
        for root in &self.roots {
            flatten_node(root, 0, &mut result);
        }
        result
    }
}


impl Default for TaskTree {
    fn default() -> Self {
        Self::new()
    }
}


fn find_node<'a>(node: &'a TaskNode, id: &str) -> Option<&'a TaskNode> {
    if node.id == id {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node(child, id) {
            return Some(found);
        }
    }
    None
}


fn find_node_mut<'a>(node: &'a mut TaskNode, id: &str) -> Option<&'a mut TaskNode> {
    if node.id == id {
        return Some(node);
    }
    for child in &mut node.children {
        if let Some(found) = find_node_mut(child, id) {
            return Some(found);
        }
    }
    None
}


fn flatten_node<'a>(node: &'a TaskNode, depth: usize, out: &mut Vec<(&'a TaskNode, usize)>) {
    out.push((node, depth));
    for child in &node.children {
        flatten_node(child, depth + 1, out);
    }
}


/// Recursively propagate status from leaves to parents.
/// Returns the effective status of the subtree rooted at `node`.
fn propagate_node(node: &mut TaskNode) -> TaskStatus {
    if node.children.is_empty() {
        return node.status.clone();
    }

    let mut child_statuses = Vec::new();
    for child in &mut node.children {
        child_statuses.push(propagate_node(child));
    }

    let all_completed = child_statuses.iter().all(|s| *s == TaskStatus::Completed);
    let any_failed = child_statuses.iter().any(|s| *s == TaskStatus::Failed);
    let any_in_progress = child_statuses
        .iter()
        .any(|s| *s == TaskStatus::InProgress);

    if all_completed {
        node.status = TaskStatus::Completed;
    } else if any_failed {
        node.status = TaskStatus::Failed;
    } else if any_in_progress {
        node.status = TaskStatus::InProgress;
    }
    // Otherwise leave parent status unchanged (e.g. all Pending stays Pending)

    node.status.clone()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::task::TaskSource;

    fn make_task(id: &str, title: &str) -> TaskNode {
        TaskNode {
            id: id.into(),
            title: title.into(),
            source: TaskSource::Roadmap,
            status: TaskStatus::Pending,
            result: None,
            agent: None,
            children: Vec::new(),
            spec_path: None,
        }
    }

    #[test]
    fn new_tree_is_empty() {
        let tree = TaskTree::new();
        assert!(tree.roots().is_empty());
    }

    #[test]
    fn add_root_and_get() {
        let mut tree = TaskTree::new();
        tree.add_root(make_task("M1", "Milestone 1"));
        assert!(tree.get("M1").is_some());
        assert_eq!(tree.get("M1").unwrap().title, "Milestone 1");
    }

    #[test]
    fn get_nested_child() {
        let mut tree = TaskTree::new();
        let mut parent = make_task("M1", "Milestone 1");
        parent.children.push(make_task("M1.1", "Section 1"));
        parent.children[0]
            .children
            .push(make_task("M1.1.1", "Leaf"));
        tree.add_root(parent);

        assert!(tree.get("M1.1").is_some());
        assert!(tree.get("M1.1.1").is_some());
        assert!(tree.get("M1.2").is_none());
    }

    #[test]
    fn get_mut_works() {
        let mut tree = TaskTree::new();
        tree.add_root(make_task("M1", "Milestone"));
        tree.get_mut("M1").unwrap().title = "Updated".into();
        assert_eq!(tree.get("M1").unwrap().title, "Updated");
    }

    #[test]
    fn set_status() {
        let mut tree = TaskTree::new();
        tree.add_root(make_task("M1", "Milestone"));
        tree.set_status("M1", TaskStatus::InProgress).unwrap();
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::InProgress);
    }

    #[test]
    fn set_status_not_found() {
        let mut tree = TaskTree::new();
        let result = tree.set_status("nope", TaskStatus::Completed);
        assert!(result.is_err());
    }

    #[test]
    fn assign_and_unassign() {
        let mut tree = TaskTree::new();
        tree.add_root(make_task("M1", "Milestone"));
        tree.assign("M1", "worker1").unwrap();
        assert_eq!(tree.get("M1").unwrap().agent.as_deref(), Some("worker1"));
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::InProgress);

        let old = tree.unassign("M1").unwrap();
        assert_eq!(old, Some("worker1".into()));
        assert!(tree.get("M1").unwrap().agent.is_none());
    }

    #[test]
    fn assign_not_found() {
        let mut tree = TaskTree::new();
        assert!(tree.assign("nope", "w1").is_err());
    }

    #[test]
    fn unassign_not_found() {
        let mut tree = TaskTree::new();
        assert!(tree.unassign("nope").is_err());
    }

    #[test]
    fn propagate_all_completed() {
        let mut tree = TaskTree::new();
        let mut parent = make_task("M1", "Milestone");
        let mut c1 = make_task("M1.1", "Child 1");
        let mut c2 = make_task("M1.2", "Child 2");
        c1.status = TaskStatus::Completed;
        c2.status = TaskStatus::Completed;
        parent.children.push(c1);
        parent.children.push(c2);
        tree.add_root(parent);

        tree.propagate_status();
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::Completed);
    }

    #[test]
    fn propagate_any_failed() {
        let mut tree = TaskTree::new();
        let mut parent = make_task("M1", "Milestone");
        let mut c1 = make_task("M1.1", "Child 1");
        let mut c2 = make_task("M1.2", "Child 2");
        c1.status = TaskStatus::Completed;
        c2.status = TaskStatus::Failed;
        parent.children.push(c1);
        parent.children.push(c2);
        tree.add_root(parent);

        tree.propagate_status();
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::Failed);
    }

    #[test]
    fn propagate_any_in_progress() {
        let mut tree = TaskTree::new();
        let mut parent = make_task("M1", "Milestone");
        let mut c1 = make_task("M1.1", "Child 1");
        let mut c2 = make_task("M1.2", "Child 2");
        c1.status = TaskStatus::InProgress;
        c2.status = TaskStatus::Pending;
        parent.children.push(c1);
        parent.children.push(c2);
        tree.add_root(parent);

        tree.propagate_status();
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::InProgress);
    }

    #[test]
    fn propagate_deep_tree() {
        let mut tree = TaskTree::new();
        let mut root = make_task("M1", "Root");
        let mut mid = make_task("M1.1", "Mid");
        let mut leaf1 = make_task("M1.1.1", "Leaf 1");
        let mut leaf2 = make_task("M1.1.2", "Leaf 2");
        leaf1.status = TaskStatus::Completed;
        leaf2.status = TaskStatus::Completed;
        mid.children.push(leaf1);
        mid.children.push(leaf2);
        root.children.push(mid);
        tree.add_root(root);

        tree.propagate_status();
        assert_eq!(tree.get("M1.1").unwrap().status, TaskStatus::Completed);
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::Completed);
    }

    #[test]
    fn flat_list_order_and_depth() {
        let mut tree = TaskTree::new();
        let mut root = make_task("M1", "Root");
        let mut child = make_task("M1.1", "Child");
        child.children.push(make_task("M1.1.1", "Grandchild"));
        root.children.push(child);
        root.children.push(make_task("M1.2", "Child 2"));
        tree.add_root(root);
        tree.add_root(make_task("M2", "Root 2"));

        let flat = tree.flat_list();
        let entries: Vec<(&str, usize)> = flat.iter().map(|(n, d)| (n.id.as_str(), *d)).collect();
        assert_eq!(
            entries,
            vec![
                ("M1", 0),
                ("M1.1", 1),
                ("M1.1.1", 2),
                ("M1.2", 1),
                ("M2", 0),
            ]
        );
    }

    #[test]
    fn flat_list_empty() {
        let tree = TaskTree::new();
        assert!(tree.flat_list().is_empty());
    }

    #[test]
    fn propagate_leaves_pending_alone() {
        let mut tree = TaskTree::new();
        let mut parent = make_task("M1", "Milestone");
        parent.children.push(make_task("M1.1", "Child 1"));
        parent.children.push(make_task("M1.2", "Child 2"));
        tree.add_root(parent);

        tree.propagate_status();
        // All children pending, parent stays pending
        assert_eq!(tree.get("M1").unwrap().status, TaskStatus::Pending);
    }
}

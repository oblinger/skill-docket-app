use crate::types::task::{TaskNode, TaskSource};

pub fn merge_task_trees(roadmap_tasks: &mut Vec<TaskNode>, filesystem_tasks: Vec<TaskNode>) {
    for fs_task in filesystem_tasks {
        if let Some(rm_task) = roadmap_tasks.iter_mut().find(|t| t.id == fs_task.id) {
            rm_task.source = TaskSource::Both;
            if rm_task.spec_path.is_none() { rm_task.spec_path = fs_task.spec_path; }
            merge_task_trees(&mut rm_task.children, fs_task.children);
        } else {
            roadmap_tasks.push(fs_task);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::task::TaskStatus;
    fn mt(id: &str, title: &str, source: TaskSource) -> TaskNode {
        TaskNode { id: id.into(), title: title.into(), source, status: TaskStatus::Pending, result: None, agent: None, children: Vec::new(), spec_path: None }
    }
    #[test] fn merge_matching_sets_both() {
        let mut rm = vec![mt("1", "RM", TaskSource::Roadmap)];
        let mut fs = mt("1", "FS", TaskSource::Filesystem); fs.spec_path = Some("/spec.md".into());
        merge_task_trees(&mut rm, vec![fs]);
        assert_eq!(rm.len(), 1); assert_eq!(rm[0].source, TaskSource::Both);
        assert_eq!(rm[0].spec_path.as_deref(), Some("/spec.md")); assert_eq!(rm[0].title, "RM");
    }
    #[test] fn merge_appends_fs_only() {
        let mut rm = vec![mt("1", "T1", TaskSource::Roadmap)];
        merge_task_trees(&mut rm, vec![mt("4", "Extra", TaskSource::Filesystem)]);
        assert_eq!(rm.len(), 2); assert_eq!(rm[1].id, "4"); assert_eq!(rm[1].source, TaskSource::Filesystem);
    }
    #[test] fn merge_recursive_children() {
        let mut rmt = mt("1", "T1", TaskSource::Roadmap); rmt.children.push(mt("1.1", "C1.1", TaskSource::Roadmap));
        let mut rm = vec![rmt];
        let mut fst = mt("1", "T1", TaskSource::Filesystem); fst.children.push(mt("1.2", "C1.2", TaskSource::Filesystem));
        merge_task_trees(&mut rm, vec![fst]);
        assert_eq!(rm[0].source, TaskSource::Both); assert_eq!(rm[0].children.len(), 2);
    }
    #[test] fn merge_preserves_ordering() {
        let mut rm = vec![mt("1", "A", TaskSource::Roadmap), mt("2", "B", TaskSource::Roadmap), mt("3", "C", TaskSource::Roadmap)];
        merge_task_trees(&mut rm, vec![mt("4", "D", TaskSource::Filesystem)]);
        assert_eq!(rm.len(), 4); assert_eq!(rm[3].id, "4");
    }
    #[test] fn merge_empty_roadmap() {
        let mut rm: Vec<TaskNode> = Vec::new();
        merge_task_trees(&mut rm, vec![mt("1", "FS", TaskSource::Filesystem)]);
        assert_eq!(rm.len(), 1);
    }
    #[test] fn merge_empty_filesystem() {
        let mut rm = vec![mt("1", "RM", TaskSource::Roadmap)];
        merge_task_trees(&mut rm, vec![]);
        assert_eq!(rm[0].source, TaskSource::Roadmap);
    }
    #[test] fn merge_keeps_existing_spec_path() {
        let mut rmt = mt("1", "T1", TaskSource::Roadmap); rmt.spec_path = Some("/rm.md".into());
        let mut rm = vec![rmt];
        let mut fst = mt("1", "T1", TaskSource::Filesystem); fst.spec_path = Some("/fs.md".into());
        merge_task_trees(&mut rm, vec![fst]);
        assert_eq!(rm[0].spec_path.as_deref(), Some("/rm.md"));
    }
}

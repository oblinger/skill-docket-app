use std::path::Path;
use crate::types::task::{TaskNode, TaskSource, TaskStatus};

pub fn scan_tasks(project_path: &Path) -> Result<Vec<TaskNode>, String> {
    scan_inner(project_path, None)
}

fn scan_inner(project_path: &Path, anchor_name: Option<&str>) -> Result<Vec<TaskNode>, String> {
    let mut tasks = Vec::new();
    let all: Vec<_> = std::fs::read_dir(project_path)
        .map_err(|e| format!("Cannot read {}: {}", project_path.display(), e))?
        .filter_map(|e| e.ok()).collect();
    let dir_names: Vec<String> = all.iter().filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string()).collect();
    for entry in &all {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some((number, title)) = parse_numbered_entry(&name) {
            let path = entry.path();
            if path.is_dir() {
                let anchor = path.join(format!("{}.md", name));
                if anchor.exists() {
                    let mut task = TaskNode { id: number.to_string(), title, source: TaskSource::Filesystem,
                        status: TaskStatus::Pending, result: None, agent: None, children: Vec::new(),
                        spec_path: Some(anchor.to_string_lossy().to_string()) };
                    if let Ok(sub) = scan_inner(&path, Some(&name)) { task.children = sub; }
                    tasks.push(task);
                }
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                let stem = name.strip_suffix(".md").unwrap_or(&name);
                if let Some(a) = anchor_name { if stem == a { continue; } }
                if dir_names.contains(&stem.to_string()) { continue; }
                tasks.push(TaskNode { id: number.to_string(), title, source: TaskSource::Filesystem,
                    status: TaskStatus::Pending, result: None, agent: None, children: Vec::new(),
                    spec_path: Some(path.to_string_lossy().to_string()) });
            }
        }
    }
    tasks.sort_by(|a, b| { let an: u32 = a.id.parse().unwrap_or(0); let bn: u32 = b.id.parse().unwrap_or(0); an.cmp(&bn) });
    Ok(tasks)
}

fn parse_numbered_entry(name: &str) -> Option<(u32, String)> {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    let pos = stem.find('_')?;
    let number: u32 = stem[..pos].parse().ok()?;
    Some((number, stem[pos + 1..].replace('_', " ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn scan_discovers_numbered_folders() {
        let dir = std::env::temp_dir().join("cmx_scan_folders"); let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t1 = dir.join("01_define"); std::fs::create_dir(&t1).unwrap(); std::fs::write(t1.join("01_define.md"), "#").unwrap();
        let t2 = dir.join("02_impl"); std::fs::create_dir(&t2).unwrap(); std::fs::write(t2.join("02_impl.md"), "#").unwrap();
        let tasks = scan_tasks(&dir).unwrap();
        assert_eq!(tasks.len(), 2); assert_eq!(tasks[0].id, "1"); assert_eq!(tasks[1].id, "2");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn scan_ignores_no_anchor() {
        let dir = std::env::temp_dir().join("cmx_scan_no_anchor"); let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap(); std::fs::create_dir(dir.join("01_x")).unwrap();
        assert!(scan_tasks(&dir).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn scan_discovers_md_files() {
        let dir = std::env::temp_dir().join("cmx_scan_md"); let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap(); std::fs::write(dir.join("03_data_model.md"), "#").unwrap();
        let tasks = scan_tasks(&dir).unwrap();
        assert_eq!(tasks.len(), 1); assert_eq!(tasks[0].id, "3"); assert_eq!(tasks[0].title, "data model");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn scan_recurses() {
        let dir = std::env::temp_dir().join("cmx_scan_recurse"); let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("01_parent"); std::fs::create_dir(&p).unwrap(); std::fs::write(p.join("01_parent.md"), "#").unwrap();
        let c = p.join("01_child"); std::fs::create_dir(&c).unwrap(); std::fs::write(c.join("01_child.md"), "#").unwrap();
        let tasks = scan_tasks(&dir).unwrap();
        assert_eq!(tasks.len(), 1); assert_eq!(tasks[0].children.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn parse_numbered_entry_formats() {
        assert_eq!(parse_numbered_entry("01_hello_world"), Some((1, "hello world".into())));
        assert_eq!(parse_numbered_entry("03_data_model.md"), Some((3, "data model".into())));
        assert_eq!(parse_numbered_entry("not_numbered"), None);
        assert_eq!(parse_numbered_entry("readme.md"), None);
    }
    #[test] fn scan_ignores_non_numbered() {
        let dir = std::env::temp_dir().join("cmx_scan_non_num"); let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap(); std::fs::write(dir.join("README.md"), "#").unwrap();
        assert!(scan_tasks(&dir).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn scan_sorts_by_number() {
        let dir = std::env::temp_dir().join("cmx_scan_sort"); let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("03_c.md"), "#").unwrap(); std::fs::write(dir.join("01_a.md"), "#").unwrap(); std::fs::write(dir.join("02_b.md"), "#").unwrap();
        let tasks = scan_tasks(&dir).unwrap();
        assert_eq!(tasks[0].id, "1"); assert_eq!(tasks[1].id, "2"); assert_eq!(tasks[2].id, "3");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test] fn scan_nonexistent_errors() { assert!(scan_tasks(Path::new("/tmp/cmx_no_exist_xyz")).is_err()); }
}

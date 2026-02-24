use crate::types::session::{LayoutEntry, LayoutNode};

pub fn parse_layout_expr(input: &str) -> Result<LayoutNode, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() { return Err("empty layout expression".into()); }
    let upper = trimmed.to_uppercase();
    if upper.starts_with("ROW(") {
        let inner = extract_parens(trimmed)?;
        Ok(LayoutNode::Row { children: parse_children(inner)? })
    } else if upper.starts_with("COL(") {
        let inner = extract_parens(trimmed)?;
        Ok(LayoutNode::Col { children: parse_children(inner)? })
    } else {
        let (name, _) = parse_leaf(trimmed)?;
        Ok(LayoutNode::Pane { agent: name })
    }
}

fn parse_children(inner: &str) -> Result<Vec<LayoutEntry>, String> {
    let parts = split_top_level_commas(inner);
    let mut entries = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() { continue; }
        let (node, percent) = parse_child_entry(part)?;
        entries.push(LayoutEntry { node, percent });
    }
    if entries.is_empty() { return Err("empty children list".into()); }
    Ok(entries)
}

fn parse_child_entry(s: &str) -> Result<(LayoutNode, Option<u32>), String> {
    let trimmed = s.trim();
    let upper = trimmed.to_uppercase();
    if upper.starts_with("ROW(") || upper.starts_with("COL(") {
        let close = find_matching_paren(trimmed)?;
        let nested_str = &trimmed[..=close];
        let remainder = trimmed[close + 1..].trim();
        let node = parse_layout_expr(nested_str)?;
        let percent = parse_trailing_percent(remainder)?;
        Ok((node, percent))
    } else {
        let (name, percent) = parse_leaf(trimmed)?;
        Ok((LayoutNode::Pane { agent: name }, percent))
    }
}

fn extract_parens(s: &str) -> Result<&str, String> {
    let open = s.find('(').ok_or_else(|| format!("expected '(' in: {}", s))?;
    let close = s.rfind(')').ok_or_else(|| format!("expected ')' in: {}", s))?;
    if close <= open { return Err(format!("mismatched parens in: {}", s)); }
    Ok(&s[open + 1..close])
}

fn find_matching_paren(s: &str) -> Result<usize, String> {
    let start = s.find('(').ok_or_else(|| format!("no open paren in: {}", s))?;
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { return Ok(i); } }
            _ => {}
        }
    }
    Err(format!("unmatched paren at {}", start))
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => { parts.push(&s[start..i]); start = i + 1; }
            _ => {}
        }
    }
    if start <= s.len() { parts.push(&s[start..]); }
    parts
}

fn parse_leaf(s: &str) -> Result<(String, Option<u32>), String> {
    let trimmed = s.trim();
    if trimmed.is_empty() { return Err("empty leaf name".into()); }
    if let Some(space_pos) = trimmed.rfind(' ') {
        let maybe_pct = trimmed[space_pos + 1..].trim();
        if let Some(num_str) = maybe_pct.strip_suffix('%') {
            if let Ok(n) = num_str.parse::<u32>() {
                let name = trimmed[..space_pos].trim().to_string();
                if name.is_empty() { return Err("empty leaf name".into()); }
                return Ok((name, Some(n)));
            }
        }
    }
    Ok((trimmed.to_string(), None))
}

fn parse_trailing_percent(s: &str) -> Result<Option<u32>, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() { return Ok(None); }
    if let Some(num_str) = trimmed.strip_suffix('%') {
        match num_str.trim().parse::<u32>() {
            Ok(n) => Ok(Some(n)),
            Err(_) => Err(format!("invalid percentage: {}", trimmed)),
        }
    } else {
        Err(format!("unexpected trailing: {}", trimmed))
    }
}

pub fn serialize_layout_expr(node: &LayoutNode) -> String {
    match node {
        LayoutNode::Row { children } => {
            let parts: Vec<String> = children.iter().map(serialize_entry).collect();
            format!("ROW({})", parts.join(", "))
        }
        LayoutNode::Col { children } => {
            let parts: Vec<String> = children.iter().map(serialize_entry).collect();
            format!("COL({})", parts.join(", "))
        }
        LayoutNode::Pane { agent } => agent.clone(),
    }
}

fn serialize_entry(entry: &LayoutEntry) -> String {
    let inner = serialize_layout_expr(&entry.node);
    match entry.percent {
        Some(p) => format!("{} {}%", inner, p),
        None => inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parse_simple_row() {
        let node = parse_layout_expr("ROW(pilot 50%, worker1 50%)").unwrap();
        match node { LayoutNode::Row { children } => { assert_eq!(children.len(), 2); assert_eq!(children[0].percent, Some(50)); } _ => panic!() }
    }
    #[test] fn parse_nested_expression() {
        let expr = "COL(ROW(pilot 50%, worker1 50%) 60%, ROW(pm 30%, worker2 70%) 40%)";
        let node = parse_layout_expr(expr).unwrap();
        match node { LayoutNode::Col { children } => { assert_eq!(children.len(), 2); assert_eq!(children[0].percent, Some(60)); } _ => panic!() }
    }
    #[test] fn round_trip_nested_expression() {
        let expr = "ROW(pilot 50%, COL(w1 60%, w2 40%) 50%)";
        let node = parse_layout_expr(expr).unwrap();
        let s = serialize_layout_expr(&node);
        assert_eq!(node, parse_layout_expr(&s).unwrap());
    }
    #[test] fn parse_single_pane() {
        match parse_layout_expr("pilot").unwrap() { LayoutNode::Pane { agent } => assert_eq!(agent, "pilot"), _ => panic!() }
    }
    #[test] fn parse_case_insensitive() {
        assert!(matches!(parse_layout_expr("row(a 50%, b 50%)").unwrap(), LayoutNode::Row { .. }));
        assert!(matches!(parse_layout_expr("col(x 30%, y 70%)").unwrap(), LayoutNode::Col { .. }));
    }
    #[test] fn parse_no_percentages() {
        match parse_layout_expr("ROW(pilot, worker1)").unwrap() { LayoutNode::Row { children } => { assert_eq!(children.len(), 2); assert_eq!(children[0].percent, None); } _ => panic!() }
    }
    #[test] fn serialize_simple_row() {
        let node = LayoutNode::Row { children: vec![
            LayoutEntry { node: LayoutNode::Pane { agent: "pilot".into() }, percent: Some(50) },
            LayoutEntry { node: LayoutNode::Pane { agent: "worker1".into() }, percent: Some(50) },
        ]};
        assert_eq!(serialize_layout_expr(&node), "ROW(pilot 50%, worker1 50%)");
    }
    #[test] fn parse_empty_errors() { assert!(parse_layout_expr("").is_err()); assert!(parse_layout_expr("   ").is_err()); }
    #[test] fn round_trip_deeply_nested() {
        let expr = "ROW(COL(a 30%, ROW(b 50%, c 50%) 70%) 40%, d 60%)";
        let node = parse_layout_expr(expr).unwrap();
        assert_eq!(node, parse_layout_expr(&serialize_layout_expr(&node)).unwrap());
    }
}

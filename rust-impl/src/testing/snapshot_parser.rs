/// Parses snapshot files (.snap) into structured test cases.
///
/// Snapshot format:
/// ```text
/// ---
/// source: ...
/// assertion_line: N
/// expression: output
/// ---
/// ==INPUT==
/// <source code>
/// ============================= <path> (ENTRY POINT)==
/// <segment code>
/// Some("<source map json>")
/// /*
/// { <segment metadata json> }
/// */
/// ============================= <path> ==
/// <parent module code>
/// Some("<source map json>")
/// == DIAGNOSTICS ==
/// [<diagnostics json>]
/// ```

#[derive(Debug, Clone)]
pub struct SnapshotTestCase {
    pub name: String,
    pub input: String,
    pub segments: Vec<SnapshotSegment>,
    pub parent_module: Option<SnapshotParentModule>,
    pub diagnostics: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SnapshotSegment {
    pub path: String,
    pub is_entry: bool,
    pub code: String,
    pub source_map: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SnapshotParentModule {
    pub path: String,
    pub code: String,
    pub source_map: Option<String>,
}

/// Parse a .snap file into a SnapshotTestCase
pub fn parse_snapshot(name: &str, content: &str) -> SnapshotTestCase {
    let mut input = String::new();
    let mut segments: Vec<SnapshotSegment> = Vec::new();
    let mut parent_module: Option<SnapshotParentModule> = None;
    let mut diagnostics: Option<String> = None;

    // Strip frontmatter (--- ... ---)
    let content = strip_frontmatter(content);

    // Split by section markers
    let sections = split_sections(&content);

    for section in &sections {
        match section {
            Section::Input(code) => {
                input = code.clone();
            }
            Section::Module { path, is_entry, code, source_map, metadata } => {
                if *is_entry {
                    segments.push(SnapshotSegment {
                        path: path.clone(),
                        is_entry: true,
                        code: code.clone(),
                        source_map: source_map.clone(),
                        metadata: metadata.clone(),
                    });
                } else if is_parent_module_path(path) {
                    // Parent module has a simple path like "test.tsx" (no underscore-hash suffix)
                    parent_module = Some(SnapshotParentModule {
                        path: path.clone(),
                        code: code.clone(),
                        source_map: source_map.clone(),
                    });
                } else {
                    // Non-entry, non-parent modules are intermediate segment modules
                    segments.push(SnapshotSegment {
                        path: path.clone(),
                        is_entry: false,
                        code: code.clone(),
                        source_map: source_map.clone(),
                        metadata: metadata.clone(),
                    });
                }
            }
            Section::Diagnostics(diag) => {
                diagnostics = Some(diag.clone());
            }
        }
    }

    SnapshotTestCase {
        name: name.to_string(),
        input,
        segments,
        parent_module,
        diagnostics,
    }
}

#[derive(Debug)]
enum Section {
    Input(String),
    Module {
        path: String,
        is_entry: bool,
        code: String,
        source_map: Option<String>,
        metadata: Option<String>,
    },
    Diagnostics(String),
}

fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }
    // Find the second ---
    if let Some(end) = trimmed[3..].find("---") {
        trimmed[3 + end + 3..].to_string()
    } else {
        content.to_string()
    }
}

fn split_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Check for ==INPUT==
        if line.trim() == "==INPUT==" {
            i += 1;
            let mut code = String::new();
            while i < lines.len() && !lines[i].starts_with("=====") && lines[i].trim() != "== DIAGNOSTICS ==" {
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(lines[i]);
                i += 1;
            }
            sections.push(Section::Input(code));
            continue;
        }

        // Check for ===== module header
        if line.starts_with("=====") {
            let (path, is_entry) = parse_module_header(line);
            i += 1;
            let mut code = String::new();
            let mut source_map: Option<String> = None;
            let mut metadata: Option<String> = None;
            let mut in_metadata = false;
            let mut metadata_buf = String::new();

            while i < lines.len() && !lines[i].starts_with("=====") && lines[i].trim() != "== DIAGNOSTICS ==" {
                let l = lines[i];

                // Check for source map: Some("...")
                if l.starts_with("Some(\"") {
                    // Source map line - extract the JSON
                    let sm = l.strip_prefix("Some(\"").and_then(|s| s.strip_suffix("\")"));
                    if let Some(sm) = sm {
                        source_map = Some(sm.replace("\\\"", "\""));
                    }
                    i += 1;
                    continue;
                }

                // Check for metadata block: /* ... */
                if l.trim() == "/*" {
                    in_metadata = true;
                    i += 1;
                    continue;
                }
                if l.trim() == "*/" {
                    in_metadata = false;
                    metadata = Some(metadata_buf.clone());
                    metadata_buf.clear();
                    i += 1;
                    continue;
                }
                if in_metadata {
                    if !metadata_buf.is_empty() {
                        metadata_buf.push('\n');
                    }
                    metadata_buf.push_str(l);
                    i += 1;
                    continue;
                }

                // Regular code line
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(l);
                i += 1;
            }

            // Trim trailing empty lines from code
            let code = code.trim_end().to_string();

            sections.push(Section::Module {
                path,
                is_entry,
                code,
                source_map,
                metadata,
            });
            continue;
        }

        // Check for diagnostics
        if line.trim() == "== DIAGNOSTICS ==" {
            i += 1;
            let mut diag = String::new();
            while i < lines.len() {
                if !diag.is_empty() {
                    diag.push('\n');
                }
                diag.push_str(lines[i]);
                i += 1;
            }
            sections.push(Section::Diagnostics(diag.trim().to_string()));
            continue;
        }

        i += 1;
    }

    sections
}

/// Parse a module header line like:
/// `============================= test.tsx_foo_bar.tsx (ENTRY POINT)==`
/// or
/// `============================= test.tsx ==`
fn parse_module_header(line: &str) -> (String, bool) {
    let is_entry = line.contains("(ENTRY POINT)");

    // Strip leading = signs
    let stripped = line.trim_start_matches('=').trim();

    // Strip trailing = signs and "(ENTRY POINT)"
    let stripped = stripped.trim_end_matches('=').trim();
    let stripped = stripped.replace("(ENTRY POINT)", "");
    let path = stripped.trim().to_string();

    (path, is_entry)
}

/// Returns true if the path looks like a parent module (e.g., "test.tsx" or "[[...slug]].js")
/// rather than a segment module (e.g., "test.tsx_slug_component_0AM8HPnkNs4.js").
/// Segment modules embed the original file extension in their filename followed by an underscore,
/// producing patterns like ".tsx_", ".ts_", ".js_", ".jsx_" in the filename portion.
fn is_parent_module_path(path: &str) -> bool {
    // Get just the filename (last path component)
    let filename = path.rsplit('/').next().unwrap_or(path);
    // Segment modules contain an embedded source extension followed by underscore
    // e.g., "[[...slug]].tsx_slug_component_0AM8HPnkNs4.js"
    let segment_patterns = [".tsx_", ".ts_", ".jsx_", ".js_"];
    !segment_patterns.iter().any(|pat| filename.contains(pat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_module_header_entry() {
        let (path, is_entry) = parse_module_header(
            "============================= test.tsx_renderHeader1_jMxQsjbyDss.tsx (ENTRY POINT)=="
        );
        assert_eq!(path, "test.tsx_renderHeader1_jMxQsjbyDss.tsx");
        assert!(is_entry);
    }

    #[test]
    fn test_parse_module_header_parent() {
        let (path, is_entry) = parse_module_header(
            "============================= test.tsx =="
        );
        assert_eq!(path, "test.tsx");
        assert!(!is_entry);
    }

    #[test]
    fn test_strip_frontmatter() {
        let input = "---\nsource: foo\n---\nhello";
        assert_eq!(strip_frontmatter(input).trim(), "hello");
    }

    #[test]
    fn test_parse_basic_snapshot() {
        let content = r#"---
source: test.rs
assertion_line: 42
expression: output
---
==INPUT==

const x = 1;

============================= test.tsx ==

const x = 1;

"#;
        let tc = parse_snapshot("test", content);
        assert_eq!(tc.input.trim(), "const x = 1;");
        assert!(tc.parent_module.is_some());
        assert_eq!(tc.parent_module.unwrap().path, "test.tsx");
    }
}

/// Parent module rewriting engine.
///
/// Uses string manipulation to surgically edit source at AST positions.
/// Replaces $() calls with QRL references, manages imports, assembles
/// final parent module.

use super::extract::ExtractionResult;
use super::marker_detection::{to_qrl_name, ImportInfo};
use super::rewrite_calls::{build_qrl_declaration, qrl_var_name};

/// Result of rewriting the parent module.
#[derive(Debug, Clone)]
pub struct ParentRewriteResult {
    pub code: String,
    pub source_map: Option<String>,
}

/// Rewrite the parent module, replacing $() calls with QRL references.
pub fn rewrite_parent_module(
    source: &str,
    extractions: &[ExtractionResult],
    symbol_names: &[String],
    canonical_filenames: &[String],
    captures: &[Vec<String>],
    imports: &[ImportInfo],
    _file_path: &str,
) -> ParentRewriteResult {
    let mut result = source.to_string();

    // We need to process replacements from end to start to preserve offsets
    let mut replacements: Vec<(u32, u32, String)> = Vec::new();

    for (i, extraction) in extractions.iter().enumerate() {
        let symbol_name = &symbol_names[i];
        let var_name = qrl_var_name(symbol_name);

        // Replace the entire $() call with the QRL variable reference
        replacements.push((extraction.call_start, extraction.call_end, var_name.clone()));
    }

    // Sort by position descending (process from end to start)
    replacements.sort_by(|a, b| b.0.cmp(&a.0));

    for (start, end, replacement) in &replacements {
        let start = *start as usize;
        let end = *end as usize;
        // Skip if out of bounds or not at char boundary (nested segments may overlap)
        if start >= result.len() || end > result.len()
            || !result.is_char_boundary(start) || !result.is_char_boundary(end)
            || start >= end
        {
            continue;
        }
        result.replace_range(start..end, replacement);
    }

    // Now build the import/declaration block
    let mut decl_block = String::new();

    // Collect which imports we need to add/modify
    let mut needs_qrl_import = false;
    let mut marker_import_removals: Vec<String> = Vec::new();

    for (i, extraction) in extractions.iter().enumerate() {
        needs_qrl_import = true;

        let qrl_decl = build_qrl_declaration(
            &symbol_names[i],
            &canonical_filenames[i],
            &captures[i],
            "tsx", // TODO: use actual extension
        );
        decl_block.push_str(&qrl_decl);
        decl_block.push_str(";\n");

        // Track which marker imports need to be rewritten
        let _qrl_name = to_qrl_name(&extraction.marker_name);
        marker_import_removals.push(extraction.marker_name.clone());
    }

    // Rewrite imports: replace marker imports with QRL imports
    // For now, do a simple string replacement of import lines
    result = rewrite_imports_in_source(&result, &marker_import_removals, needs_qrl_import, imports);

    // Insert QRL declarations after imports
    let insert_pos = find_import_end(&result);
    let mut final_code = String::new();
    final_code.push_str(&result[..insert_pos]);
    if !decl_block.is_empty() {
        final_code.push_str("//\n");
        final_code.push_str(&decl_block);
        final_code.push_str("//\n");
    }
    final_code.push_str(&result[insert_pos..]);

    ParentRewriteResult {
        code: final_code,
        source_map: None,
    }
}

/// Rewrite import declarations, replacing $ names with Qrl names.
fn rewrite_imports_in_source(
    source: &str,
    marker_removals: &[String],
    needs_qrl: bool,
    _imports: &[ImportInfo],
) -> String {
    let mut result = String::new();
    let mut added_qrl = false;

    for line in source.lines() {
        let trimmed = line.trim();

        // Check if this is an import line that needs rewriting
        if trimmed.starts_with("import ") {
            let mut modified_line = line.to_string();
            let mut has_remaining_imports = true;

            for marker in marker_removals {
                // Replace marker$ with markerQrl in import
                if modified_line.contains(marker) {
                    let qrl_name = to_qrl_name(marker);
                    // Handle bare $ import
                    if marker == "$" {
                        // Remove `$` and `, $` and `$ ,` patterns
                        modified_line = modified_line.replace(", $", "");
                        modified_line = modified_line.replace("$ ,", "");
                        modified_line = modified_line.replace("$", "");

                        // Check if import is now empty
                        if modified_line.contains("{ }") || modified_line.contains("{}") {
                            has_remaining_imports = false;
                        }
                    } else {
                        modified_line = modified_line.replace(marker, &qrl_name);
                    }
                }
            }

            if has_remaining_imports {
                // Add qrl import if needed and from @qwik.dev/core
                if needs_qrl && !added_qrl && modified_line.contains("@qwik.dev/core") {
                    // Add qrl to the import
                    if !modified_line.contains("qrl") {
                        modified_line = modified_line.replace("{ ", "{ qrl, ");
                        if !modified_line.contains("{ qrl") {
                            modified_line = modified_line.replace("{", "{ qrl, ");
                        }
                    }
                    added_qrl = true;
                }
                result.push_str(&modified_line);
                result.push('\n');
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    // If we need qrl but haven't added it yet (no @qwik.dev/core import existed)
    if needs_qrl && !added_qrl {
        let import_line = "import { qrl } from \"@qwik.dev/core\";\n";
        result = format!("{}{}", import_line, result);
    }

    result
}

/// Find the end position of all import statements.
fn find_import_end(source: &str) -> usize {
    let mut last_import_end = 0;

    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") {
            // Find the end of this line in the original string
            let line_end: usize = source.lines().take(i + 1).map(|l| l.len() + 1).sum();
            last_import_end = line_end;
        }
    }

    last_import_end
}

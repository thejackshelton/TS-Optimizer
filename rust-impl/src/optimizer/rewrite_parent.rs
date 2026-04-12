/// Parent module rewriting engine.
///
/// Uses string manipulation to surgically edit source at AST positions.
/// Replaces $() calls with QRL references, manages imports, assembles
/// final parent module.

use std::collections::BTreeSet;

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
    if extractions.is_empty() {
        return ParentRewriteResult {
            code: source.to_string(),
            source_map: None,
        };
    }

    // ---------------------------------------------------------------
    // Step 1: Replace $() call expressions in the source text.
    //
    // For marker calls like `component$(fn)`, replace the entire call
    // with `/*#__PURE__*/ componentQrl(q_varname)`.
    // For bare `$(fn)`, replace with just `q_varname`.
    // ---------------------------------------------------------------
    let mut result = source.to_string();
    let mut replacements: Vec<(u32, u32, String)> = Vec::new();

    for (i, extraction) in extractions.iter().enumerate() {
        // Only handle top-level extractions (no parent) in the parent module.
        // Nested extractions are handled in their own segment files.
        if extraction.parent_segment.is_some() {
            continue;
        }

        let symbol_name = &symbol_names[i];
        let var_name = qrl_var_name(symbol_name);
        let marker = &extraction.marker_name;

        let replacement = if marker == "$" {
            // Bare $() call: replace with just the QRL variable
            var_name
        } else {
            // Named marker like component$, globalAction$:
            // Replace `component$(fn)` with `componentQrl(q_var)`
            // Only component$ gets /*#__PURE__*/ annotation
            let qrl_fn = to_qrl_name(marker);
            if marker == "component$" {
                format!("/*#__PURE__*/ {}({})", qrl_fn, var_name)
            } else {
                format!("{}({})", qrl_fn, var_name)
            }
        };

        replacements.push((extraction.call_start, extraction.call_end, replacement));
    }

    // Sort by position descending so we can replace from end to start
    // without invalidating earlier offsets
    replacements.sort_by(|a, b| b.0.cmp(&a.0));

    for (start, end, replacement) in &replacements {
        let start = *start as usize;
        let end = *end as usize;
        if start >= result.len() || end > result.len()
            || !result.is_char_boundary(start) || !result.is_char_boundary(end)
            || start >= end
        {
            continue;
        }
        result.replace_range(start..end, replacement);
    }

    // ---------------------------------------------------------------
    // Step 2: Build QRL declaration block.
    //
    // Each extraction gets a const declaration like:
    //   const q_Name_hash = /*#__PURE__*/ qrl(()=>import("./file_Name_hash"), "Name_hash");
    // ---------------------------------------------------------------
    let mut decl_lines: Vec<String> = Vec::new();

    for (i, extraction) in extractions.iter().enumerate() {
        if extraction.parent_segment.is_some() {
            continue;
        }

        let qrl_decl = build_qrl_declaration(
            &symbol_names[i],
            &canonical_filenames[i],
            &captures[i],
            "tsx", // TODO: use actual extension
        );
        decl_lines.push(format!("{};", qrl_decl));
    }

    // ---------------------------------------------------------------
    // Step 3: Build new import lines.
    //
    // SWC adds imports in encounter order: for each extraction (in
    // source order), it adds the Qrl import name, then `qrl`.
    // Each import is on its own line with double quotes.
    // ---------------------------------------------------------------
    let new_imports = build_new_imports(extractions, imports);

    // ---------------------------------------------------------------
    // Step 4: Rewrite original import lines.
    //
    // Remove marker specifiers ($, component$, etc.) from original
    // import lines. If all specifiers in a line were markers, remove
    // the entire line. Keep non-marker specifiers (useStore, etc.).
    // ---------------------------------------------------------------
    let marker_names: Vec<&str> = extractions.iter()
        .filter(|e| e.parent_segment.is_none())
        .map(|e| e.marker_name.as_str())
        .collect();
    let rewritten_source = rewrite_original_imports(&result, &marker_names, imports);

    // ---------------------------------------------------------------
    // Step 5: Assemble final output.
    //
    // Layout:
    //   [new imports]
    //   [rewritten original source with call replacements]
    //     (with QRL declarations inserted after original imports)
    // ---------------------------------------------------------------
    let import_end = find_import_end(&rewritten_source);

    let mut final_code = String::new();

    // Add new imports at the very top
    for imp_line in &new_imports {
        final_code.push_str(imp_line);
        final_code.push('\n');
    }

    // Original source up to end of imports
    final_code.push_str(&rewritten_source[..import_end]);

    // QRL declaration block between // delimiters
    if !decl_lines.is_empty() {
        final_code.push_str("//\n");
        for decl in &decl_lines {
            final_code.push_str(decl);
            final_code.push('\n');
        }
        final_code.push_str("//\n");
    }

    // Rest of source after imports
    final_code.push_str(&rewritten_source[import_end..]);

    ParentRewriteResult {
        code: final_code,
        source_map: None,
    }
}

/// Build the list of new import lines to add at the top of the file.
///
/// SWC adds imports in encounter order: for each extraction (in source
/// order), it emits the marker's Qrl import, then `qrl`. Duplicates
/// are suppressed. Each import gets its own line with double quotes.
fn build_new_imports(
    extractions: &[ExtractionResult],
    imports: &[ImportInfo],
) -> Vec<String> {
    // Ordered list of (specifier, source) pairs, deduplicated.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut ordered: Vec<(String, String)> = Vec::new();

    for extraction in extractions {
        if extraction.parent_segment.is_some() {
            continue;
        }

        let marker = &extraction.marker_name;

        if marker != "$" {
            // Named marker: add its Qrl import (e.g., componentQrl, globalActionQrl)
            let qrl_name = to_qrl_name(marker);
            let source = find_import_source(marker, imports);
            let key = (qrl_name.clone(), source.clone());
            if seen.insert(key) {
                ordered.push((qrl_name, source));
            }
        }

        // Always need `qrl` from @qwik.dev/core
        let qrl_key = ("qrl".to_string(), "@qwik.dev/core".to_string());
        if seen.insert(qrl_key) {
            ordered.push(("qrl".to_string(), "@qwik.dev/core".to_string()));
        }
    }

    ordered
        .iter()
        .map(|(name, source)| format!("import {{ {} }} from \"{}\";", name, source))
        .collect()
}

/// Rewrite original import lines to remove marker specifiers.
///
/// - Marker specifiers (`$`, `component$`, etc.) are stripped from
///   their import lines.
/// - If an import line has no remaining specifiers after removal,
///   the entire line is dropped.
/// - Non-marker specifiers (e.g., `useStore`) are preserved with
///   original formatting and quotes.
fn rewrite_original_imports(
    source: &str,
    marker_names: &[&str],
    imports: &[ImportInfo],
) -> String {
    // Collect ALL $-suffixed specifiers as markers to remove.
    // The SWC strips all marker imports, not just extracted ones.
    let mut local_markers: BTreeSet<String> = BTreeSet::new();
    for imp in imports {
        if imp.specifier.ends_with('$') || imp.specifier == "$" {
            local_markers.insert(imp.local_name.clone());
            local_markers.insert(imp.specifier.clone());
        }
    }
    // Also add explicitly extracted markers
    for &marker in marker_names {
        local_markers.insert(marker.to_string());
    }

    let mut result = String::new();

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("import ") && !trimmed.starts_with("import type ") {
            // Try to rewrite this import line
            if let Some(rewritten) = rewrite_import_line(line, &local_markers) {
                result.push_str(&rewritten);
                result.push('\n');
            }
            // If None, the line is dropped entirely
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Rewrite a single import line, removing specified names.
///
/// Returns `None` if all specifiers were removed (drop the line).
/// Returns `Some(line)` with markers removed if some specifiers remain.
fn rewrite_import_line(line: &str, markers: &BTreeSet<String>) -> Option<String> {
    // Parse the import line to find the specifier block { ... }
    let brace_start = line.find('{')?;
    let brace_end = line.find('}')?;
    let specifier_block = &line[brace_start + 1..brace_end];

    // Split specifiers and filter out markers
    let specifiers: Vec<&str> = specifier_block
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let remaining: Vec<&str> = specifiers
        .iter()
        .filter(|s| {
            // Handle "Foo as Bar" syntax: check both the imported name and alias
            let name = if let Some(idx) = s.find(" as ") {
                s[..idx].trim()
            } else {
                s
            };
            !markers.contains(name)
        })
        .copied()
        .collect();

    if remaining.is_empty() {
        // All specifiers were markers -- drop the entire line
        return None;
    }

    if remaining.len() == specifiers.len() {
        // Nothing was removed -- keep line as-is
        return Some(line.to_string());
    }

    // Reconstruct the line with remaining specifiers
    let new_specifiers = remaining.join(", ");
    let before_brace = &line[..brace_start];
    let after_brace = &line[brace_end + 1..];
    Some(format!("{}{{ {} }}{}", before_brace, new_specifiers, after_brace))
}

/// Find the source module that a marker function was imported from.
fn find_import_source(marker_name: &str, imports: &[ImportInfo]) -> String {
    for imp in imports {
        if imp.specifier == marker_name || imp.local_name == marker_name {
            return imp.source.clone();
        }
    }
    // Default to @qwik.dev/core if not found
    "@qwik.dev/core".to_string()
}

/// Find the byte offset where all import statements end in the source.
fn find_import_end(source: &str) -> usize {
    let mut last_import_end = 0;
    let mut pos = 0;

    for line in source.lines() {
        let next_pos = pos + line.len() + 1; // +1 for newline
        let trimmed = line.trim();
        if trimmed.starts_with("import ") || trimmed.starts_with("import\t") {
            last_import_end = next_pos.min(source.len());
        }
        pos = next_pos;
    }

    last_import_end
}

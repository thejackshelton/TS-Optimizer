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
    segment_extensions: &[String],
    explicit_extensions: bool,
) -> ParentRewriteResult {
    if extractions.is_empty() {
        return ParentRewriteResult {
            code: source.to_string(),
            source_map: None,
        };
    }

    // Pre-compute: which markers have import lines with non-marker specifiers?
    // SWC preserves `const X =` for named markers only when the original
    // import line containing the marker also has non-marker specifiers
    // (e.g., `import { component$, useStore } from '...'`). When the
    // import line has only markers, the variable declaration is stripped.
    let mut all_marker_names: BTreeSet<String> = BTreeSet::new();
    for imp in imports {
        if imp.specifier.ends_with('$') || imp.specifier == "$" {
            all_marker_names.insert(imp.specifier.clone());
            all_marker_names.insert(imp.local_name.clone());
        }
    }
    // Check each import line in the source for non-marker specifiers.
    // Build a set of markers whose import lines have non-marker specifiers.
    let mut markers_with_preserved_imports: BTreeSet<String> = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") && !trimmed.starts_with("import type ") {
            if has_non_marker_specifiers(trimmed, &all_marker_names) {
                // This import line has non-marker specifiers -- mark all
                // marker specifiers on this line as "preserved"
                if let (Some(bs), Some(be)) = (trimmed.find('{'), trimmed.find('}')) {
                    for spec in trimmed[bs+1..be].split(',') {
                        let name = spec.trim();
                        let name = if let Some(idx) = name.find(" as ") {
                            name[..idx].trim()
                        } else { name };
                        if all_marker_names.contains(name) {
                            markers_with_preserved_imports.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // Step 1: Replace $() call expressions in the source text.
    //
    // For marker calls like `component$(fn)`, replace the entire call
    // with `/*#__PURE__*/ componentQrl(q_varname)`.
    // For bare `$(fn)`, replace with just `q_varname`.
    // ---------------------------------------------------------------
    let mut result = source.to_string();
    let mut replacements: Vec<(u32, u32, String, String)> = Vec::new(); // (start, end, replacement, marker_name)
    // Track extraction indices that are inlined (no separate QRL declaration needed)
    let mut inlined_indices: BTreeSet<usize> = BTreeSet::new();
    // Track extraction indices that are nested within another extraction's extra args
    // (e.g., valiForm$() inside formAction$(..., valiForm$(...)))
    // These are handled inline and should NOT get separate replacements.
    let mut nested_in_extra_args: BTreeSet<usize> = BTreeSet::new();

    for (i, extraction) in extractions.iter().enumerate() {
        // Only handle top-level extractions (no parent) in the parent module.
        // Nested extractions are handled in their own segment files.
        if extraction.parent_segment.is_some() {
            continue;
        }

        let symbol_name = &symbol_names[i];
        let var_name = qrl_var_name(symbol_name);
        let marker = &extraction.marker_name;

        // Check if this bare $() call is in a non-exported variable declaration.
        // If so, we inline the qrl() call directly instead of using a separate
        // const q_ declaration + variable reference.
        let is_bare_dollar_in_non_exported = if marker == "$" {
            let call_start = extraction.call_start as usize;
            let before = &source[..call_start];
            let trimmed = before.trim_end();
            if trimmed.ends_with('=') {
                let line_start = before.rfind('\n').map_or(0, |p| p + 1);
                let prefix = source[line_start..call_start].trim();
                let brace_depth: i32 = source[..line_start].chars()
                    .map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
                brace_depth == 0
                    && !prefix.starts_with("export ")
                    && (prefix.starts_with("const ") || prefix.starts_with("let ") || prefix.starts_with("var "))
            } else {
                false
            }
        } else {
            false
        };

        let replacement = if marker == "$" {
            if is_bare_dollar_in_non_exported {
                // Inline the qrl() call directly
                inlined_indices.insert(i);
                let ext_for_qrl = if explicit_extensions {
                    segment_extensions.get(i).map(|s| s.as_str()).unwrap_or("")
                } else { "" };
                let qrl_decl = build_qrl_declaration(
                    &symbol_names[i],
                    &canonical_filenames[i],
                    &captures[i],
                    ext_for_qrl,
                );
                // build_qrl_declaration returns "const q_name = /*#__PURE__*/ qrl(...)"
                // We only want the "/*#__PURE__*/ qrl(...)" part
                if let Some(pos) = qrl_decl.find("/*#__PURE__*/") {
                    qrl_decl[pos..].to_string()
                } else {
                    var_name
                }
            } else {
                // Bare $() call: replace with just the QRL variable
                var_name
            }
        } else {
            // Named marker like component$, globalAction$:
            // Replace `component$(fn)` with `componentQrl(q_var)`
            // Only component$ gets /*#__PURE__*/ annotation
            let qrl_fn = to_qrl_name(marker);

            // Check for extra arguments after the first (body) argument.
            // e.g., component$(fn, { tagName: "my-foo" }) -> the text between
            // body end and call end contains ", { tagName: ... })"
            // Also handles nested $() calls within extra args (e.g.,
            // formAction$(fn, valiForm$(schema)) → formActionQrl(q_fn, valiFormQrl(q_schema)))
            let extra_args = {
                let after_body = extraction.end as usize;
                let call_end = extraction.call_end as usize;
                if after_body < call_end && call_end <= source.len() {
                    let between = &source[after_body..call_end];
                    // Find the first comma after the body arg
                    if let Some(comma_pos) = between.find(',') {
                        // Everything from after the comma to before the closing paren
                        let rest = between[comma_pos + 1..].trim();
                        // Strip the trailing )
                        let rest = rest.strip_suffix(')').unwrap_or(rest).trim();
                        if !rest.is_empty() {
                            // Remove trailing commas before closing braces
                            // (SWC reprints without trailing commas)
                            let mut cleaned = rest.replace(",\n}", "\n}").replace(", }", " }");
                            // Replace any nested $() calls within extra args.
                            // E.g., valiForm$(FeatureSchema) → valiFormQrl(q_...)
                            let extra_abs_start = after_body + comma_pos + 1;
                            // Collect nested extractions within this range
                            let mut nested_repls: Vec<(usize, usize, String)> = Vec::new();
                            for (k, other) in extractions.iter().enumerate() {
                                if k == i { continue; }
                                let ocs = other.call_start as usize;
                                let oce = other.call_end as usize;
                                if ocs >= extra_abs_start && oce <= call_end {
                                    let other_var = qrl_var_name(&symbol_names[k]);
                                    let other_marker = &other.marker_name;
                                    let other_repl = if other_marker == "$" {
                                        other_var
                                    } else {
                                        let qrl_fn = to_qrl_name(other_marker);
                                        format!("{}({})", qrl_fn, other_var)
                                    };
                                    nested_repls.push((ocs, oce, other_repl));
                                    nested_in_extra_args.insert(k);
                                }
                            }
                            // Apply nested replacements from end to start (relative to extra_abs_start)
                            nested_repls.sort_by(|a, b| b.0.cmp(&a.0));
                            for (ns, ne, nrepl) in nested_repls {
                                let rel_s = ns - extra_abs_start;
                                let rel_e = ne - extra_abs_start;
                                // Adjust for trimming of `rest`
                                let trim_offset = between[comma_pos + 1..].len() - between[comma_pos + 1..].trim_start().len();
                                if rel_s >= trim_offset && rel_e - trim_offset <= cleaned.len() {
                                    let adj_s = rel_s - trim_offset;
                                    let adj_e = rel_e - trim_offset;
                                    // Also strip the trailing `)` we already stripped
                                    if adj_e <= cleaned.len() {
                                        cleaned = format!("{}{}{}", &cleaned[..adj_s], nrepl, &cleaned[adj_e..]);
                                    }
                                }
                            }
                            Some(cleaned)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if marker == "component$" {
                if let Some(ref extra) = extra_args {
                    format!("/*#__PURE__*/ {}({}, {})", qrl_fn, var_name, extra)
                } else {
                    format!("/*#__PURE__*/ {}({})", qrl_fn, var_name)
                }
            } else if let Some(ref extra) = extra_args {
                format!("{}({}, {})", qrl_fn, var_name, extra)
            } else {
                format!("{}({})", qrl_fn, var_name)
            }
        };

        // Skip extractions that were already handled as nested within another
        // extraction's extra args (e.g., valiForm$ inside formAction$(..., valiForm$(...)))
        if nested_in_extra_args.contains(&i) {
            continue;
        }
        replacements.push((extraction.call_start, extraction.call_end, replacement, marker.clone()));
    }

    // Sort by position descending so we can replace from end to start
    // without invalidating earlier offsets
    replacements.sort_by(|a, b| b.0.cmp(&a.0));

    for (start, end, replacement, marker_name) in &replacements {
        let start = *start as usize;
        let end = *end as usize;
        if start >= result.len() || end > result.len()
            || !result.is_char_boundary(start) || !result.is_char_boundary(end)
            || start >= end
        {
            continue;
        }

        // Check if this call is the initializer of a non-exported variable
        // declaration (e.g., `const X = $(...)` without `export`).
        // SWC strips the `const X = ` prefix for bare $() calls and for
        // named markers whose import line has only marker specifiers.
        // Named markers whose import line has non-marker specifiers keep
        // the variable declaration (the import line is preserved as-is).
        let marker_has_preserved_import = markers_with_preserved_imports.contains(marker_name.as_str());
        let (actual_start, actual_replacement) =
            strip_non_exported_var_decl(&result, start, replacement, marker_name, marker_has_preserved_import);

        // Check if the call expression is a complete expression statement that
        // needs a trailing semicolon. SWC always terminates replaced top-level
        // expressions with `;`. We add one when the next non-whitespace char
        // after the call is a newline or EOF (i.e., the call is a standalone
        // statement without `;`). We must NOT add `;` when the call is nested
        // inside another expression (e.g., next char is `)`, `,`, etc.).
        let next_char = result[end..].chars().find(|c| !c.is_whitespace() || *c == '\n');
        let needs_semi = match next_char {
            None => true,          // end of file
            Some('\n') => true,    // newline = end of expression statement
            Some(';') => false,    // already has semicolon
            _ => false,            // nested inside another expression
        };
        if needs_semi {
            result.replace_range(actual_start..end, &format!("{};", actual_replacement));
        } else {
            result.replace_range(actual_start..end, &actual_replacement);
        }
    }

    // ---------------------------------------------------------------
    // Step 2: Build QRL declaration block.
    //
    // Each extraction gets a const declaration like:
    //   const q_Name_hash = /*#__PURE__*/ qrl(()=>import("./file_Name_hash"), "Name_hash");
    // ---------------------------------------------------------------
    let mut decl_lines: Vec<String> = Vec::new();

    // Collect declarations with their source positions for sorting
    let mut decl_with_pos: Vec<(u32, String)> = Vec::new();
    for (i, extraction) in extractions.iter().enumerate() {
        if extraction.parent_segment.is_some() {
            continue;
        }
        // Skip inlined extractions -- their qrl() call is already in the body
        if inlined_indices.contains(&i) {
            continue;
        }

        let ext_for_qrl = if explicit_extensions {
            segment_extensions.get(i).map(|s| s.as_str()).unwrap_or("")
        } else { "" };
        let qrl_decl = build_qrl_declaration(
            &symbol_names[i],
            &canonical_filenames[i],
            &captures[i],
            ext_for_qrl,
        );
        decl_with_pos.push((extraction.call_start, format!("{};", qrl_decl)));
    }

    // SWC orders QRL declarations alphabetically by symbol name.
    decl_with_pos.sort_by(|a, b| a.1.cmp(&b.1));
    decl_lines = decl_with_pos.into_iter().map(|(_, decl)| decl).collect();

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
    // Determine which named markers are in non-exported declarations.
    // SWC keeps the original import line intact when a named marker
    // (component$, etc.) is used in a non-exported variable declaration,
    // because the variable is preserved (not stripped like bare $()).
    let mut has_non_exported_named_marker = false;
    for extraction in extractions.iter().filter(|e| e.parent_segment.is_none()) {
        let marker = &extraction.marker_name;
        if marker == "$" {
            continue;
        }
        let call_start = extraction.call_start as usize;
        let before = &source[..call_start];
        let trimmed = before.trim_end();
        if trimmed.ends_with('=') {
            let line_start = before.rfind('\n').map_or(0, |p| p + 1);
            let prefix = source[line_start..call_start].trim();
            let brace_depth: i32 = source[..line_start].chars()
                .map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
            if brace_depth == 0
                && !prefix.starts_with("export ")
                && (prefix.starts_with("const ") || prefix.starts_with("let ") || prefix.starts_with("var "))
            {
                has_non_exported_named_marker = true;
            }
        }
    }

    let marker_names: Vec<&str> = extractions.iter()
        .filter(|e| e.parent_segment.is_none())
        .map(|e| e.marker_name.as_str())
        .collect();
    // SWC strips marker specifiers but preserves non-marker specifiers
    // that share an import source with markers. This means lines like
    // `import { component$, useStore } from '@qwik.dev/core'` become
    // `import { useStore } from '@qwik.dev/core'` (markers stripped, rest kept).
    // Lines with ONLY markers are removed entirely.
    // However, when a named marker (component$, etc.) is used in a
    // non-exported variable declaration, SWC preserves the original
    // import line intact (including the marker specifiers).
    let rewritten_source = rewrite_original_imports(
        &result, &marker_names, imports, has_non_exported_named_marker,
    );

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
    } else if !inlined_indices.is_empty() {
        // When all extractions are inlined, still emit a // separator
        // between imports and body (matching SWC behavior)
        final_code.push_str("//\n");
    }

    // Rest of source after imports, with arrow function minification
    // Also strip @qwik-disable directive comments (authoring-time only)
    let body_text = &rewritten_source[import_end..];
    let body_no_directives = strip_qwik_directive_comments_parent(body_text);
    let body_minified = super::segment_codegen::minify_arrow_public(&body_no_directives);
    // Ensure return statements end with semicolons (SWC reprints AST with semicolons)
    let body_with_semis = super::segment_codegen::ensure_return_semicolons_public(&body_minified);
    // Strip `const/let/var X = ` prefix for non-exported, unused variable declarations
    // at module level. SWC does this as DCE: if X is never referenced after extraction,
    // the declaration becomes a bare expression statement.
    let body_stripped = strip_unused_var_decl_prefixes(&body_with_semis);
    final_code.push_str(&body_stripped);

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

        // Only add Qrl imports for function-call markers (not JSX prop markers like onClick$).
        // JSX prop markers are handled as event handler QRLs, not function imports.
        if marker != "$" && !extraction.in_jsx {
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

/// Rewrite original import lines to remove marker specifiers and unused imports.
///
/// - Marker specifiers (`$`, `component$`, etc.) are stripped from
///   their import lines.
/// - Import specifiers whose local names don't appear in the rewritten
///   source (after $() call replacements) are also stripped.
/// - If an import line has no remaining specifiers after removal,
///   the entire line is dropped.
/// - Non-marker specifiers (e.g., `useStore`) are preserved with
///   original formatting and quotes.
fn rewrite_original_imports(
    source: &str,
    marker_names: &[&str],
    imports: &[ImportInfo],
    _preserve_marker_imports: bool,
) -> String {
    // Collect ALL $-suffixed specifiers as markers to remove.
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

    // Collect the non-import portion of the source to check for usage.
    let non_import_source: String = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("import ") || trimmed.starts_with("import\t"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Find import specifiers that are unused in the non-import source
    let mut unused_names: BTreeSet<String> = BTreeSet::new();
    for imp in imports {
        // Skip markers -- they're already handled separately
        if imp.specifier.ends_with('$') || imp.specifier == "$" {
            continue;
        }
        if !is_identifier_used(&imp.local_name, &non_import_source) {
            unused_names.insert(imp.local_name.clone());
            unused_names.insert(imp.specifier.clone());
        }
    }

    // Merge markers and unused into a single set of names to strip
    let mut names_to_strip = local_markers.clone();
    names_to_strip.extend(unused_names);

    let mut result = String::new();

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("import ") && !trimmed.starts_with("import type ") {
            // SWC preserves the entire original import line (including markers
            // and unused specifiers) when:
            // 1. There's a non-exported named marker var decl (preserve_marker_imports), AND
            // 2. The import line contains non-marker specifiers alongside markers
            // This handles cases like `import { component$, useStore } from '...'`
            // where `const Header = component$(...)` is non-exported.
            if _preserve_marker_imports && has_non_marker_specifiers(trimmed, &local_markers) {
                result.push_str(line);
                result.push('\n');
            } else {
                // Strip markers AND unused specifiers
                if let Some(rewritten) = rewrite_import_line(line, &names_to_strip) {
                    result.push_str(&rewritten);
                    result.push('\n');
                }
                // If None, the line is dropped entirely
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Check if an identifier name appears as a standalone word in the source text.
///
/// Returns true if `name` appears in `source` at a word boundary (not part of
/// a larger identifier). This is a simple heuristic: check that the character
/// before and after the match (if any) is not an identifier character.
fn is_identifier_used(name: &str, source: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(pos) = source[start..].find(name) {
        let abs_pos = start + pos;
        let end_pos = abs_pos + name.len();

        // Check character before: must not be alphanumeric or underscore
        let before_ok = if abs_pos == 0 {
            true
        } else {
            let ch = source[..abs_pos].chars().last().unwrap();
            !ch.is_alphanumeric() && ch != '_' && ch != '$'
        };

        // Check character after: must not be alphanumeric or underscore
        let after_ok = if end_pos >= source.len() {
            true
        } else {
            let ch = source[end_pos..].chars().next().unwrap();
            !ch.is_alphanumeric() && ch != '_' && ch != '$'
        };

        if before_ok && after_ok {
            return true;
        }

        start = abs_pos + 1;
        if start >= source.len() {
            break;
        }
    }
    false
}

/// Check if an import line contains any non-marker specifiers.
///
/// SWC preserves the entire original import line (including markers) when
/// it has non-marker specifiers. Lines with only marker specifiers are dropped.
fn has_non_marker_specifiers(line: &str, markers: &BTreeSet<String>) -> bool {
    let brace_start = match line.find('{') { Some(p) => p, None => return false };
    let brace_end = match line.find('}') { Some(p) => p, None => return false };
    let specifier_block = &line[brace_start + 1..brace_end];

    specifier_block
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .any(|s| {
            let name = if let Some(idx) = s.find(" as ") {
                s[..idx].trim()
            } else {
                s
            };
            !markers.contains(name)
        })
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
/// Applies legacy @builder.io -> @qwik.dev renaming.
fn find_import_source(marker_name: &str, imports: &[ImportInfo]) -> String {
    for imp in imports {
        if imp.specifier == marker_name || imp.local_name == marker_name {
            return rename_legacy_package(&imp.source);
        }
    }
    // Default to @qwik.dev/core if not found
    "@qwik.dev/core".to_string()
}

/// Rename legacy @builder.io package paths to @qwik.dev equivalents.
/// Handles both exact matches and subpath imports (e.g., `@builder.io/qwik/build`).
pub fn rename_legacy_package(source: &str) -> String {
    // Exact matches
    match source {
        "@builder.io/qwik" => return "@qwik.dev/core".to_string(),
        "@builder.io/qwik-city" => return "@qwik.dev/router".to_string(),
        "@builder.io/qwik-react" => return "@qwik.dev/react".to_string(),
        _ => {}
    }
    // Subpath matches: @builder.io/qwik/subpath -> @qwik.dev/core/subpath
    if let Some(rest) = source.strip_prefix("@builder.io/qwik/") {
        return format!("@qwik.dev/core/{}", rest);
    }
    if let Some(rest) = source.strip_prefix("@builder.io/qwik-city/") {
        return format!("@qwik.dev/router/{}", rest);
    }
    if let Some(rest) = source.strip_prefix("@builder.io/qwik-react/") {
        return format!("@qwik.dev/react/{}", rest);
    }
    source.to_string()
}

/// Check if a call expression at `call_start` is the initializer of a
/// non-exported variable declaration (e.g., `const X = component$(...)`
/// without `export`). If so, returns an expanded start position that
/// covers the `const X = ` prefix, so it can be stripped.
///
/// Returns `(actual_start, replacement)` where `actual_start` may be
/// earlier than `call_start` if the declaration prefix was stripped.
fn strip_non_exported_var_decl<'a>(
    source: &str,
    call_start: usize,
    replacement: &'a str,
    _marker_name: &str,
    marker_has_preserved_import: bool,
) -> (usize, std::borrow::Cow<'a, str>) {
    // SWC strips the `const X = ` prefix for non-exported declarations
    // when the marker's original import line has only marker specifiers
    // (the import is dropped, making the variable dead code).
    // When the import line has non-marker specifiers (and is preserved),
    // the variable declaration is kept.
    if marker_has_preserved_import {
        return (call_start, std::borrow::Cow::Borrowed(replacement));
    }

    // Look backwards from call_start to find the beginning of this statement.
    // We scan backwards past whitespace and `= ` to find `const/let/var IDENT`.
    let before = &source[..call_start];

    // Trim trailing whitespace before the call
    let trimmed_before = before.trim_end();

    // Check if the text before the call ends with `=` (assignment)
    if !trimmed_before.ends_with('=') {
        return (call_start, std::borrow::Cow::Borrowed(replacement));
    }

    // Now look for `const/let/var IDENT =` pattern
    // Find the start of the line containing this declaration
    let line_start = before.rfind('\n').map_or(0, |pos| pos + 1);
    let line_prefix = source[line_start..call_start].trim();

    // Check if this line starts with `export`
    if line_prefix.starts_with("export ") {
        // Exported declaration: keep the `const X = ` prefix
        return (call_start, std::borrow::Cow::Borrowed(replacement));
    }

    // Check if it's a variable declaration pattern: const/let/var IDENT =
    if line_prefix.starts_with("const ") || line_prefix.starts_with("let ")
        || line_prefix.starts_with("var ")
    {
        // Only strip if this declaration is at module level (not inside a
        // function body). We approximate this by checking if the brace
        // depth at `line_start` is zero.
        let brace_depth: i32 = source[..line_start]
            .chars()
            .map(|c| match c { '{' => 1, '}' => -1, _ => 0 })
            .sum();
        if brace_depth == 0 {
            // Strip the entire `const X = ` prefix by expanding the replacement
            // range to start at the beginning of the line
            return (line_start, std::borrow::Cow::Owned(replacement.to_string()));
        }
    }

    (call_start, std::borrow::Cow::Borrowed(replacement))
}

/// Strip `const/let/var X = ` prefixes for non-exported, unused variable
/// declarations at module level. SWC performs this as DCE: when a variable
/// is never referenced after its $() body is extracted, the declaration
/// becomes a bare expression statement (e.g., `component$(...)` → `componentQrl(q_...)`
/// without the `const X = ` prefix).
fn strip_unused_var_decl_prefixes(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut result = String::with_capacity(body.len());

    // First pass: collect all identifiers that are declared at module level
    // in non-exported `const/let/var` declarations
    let mut decl_names: Vec<(usize, String)> = Vec::new(); // (line_index, name)
    let mut brace_depth: i32 = 0;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // Track brace depth (simplified — doesn't handle strings/comments)
        for ch in trimmed.chars() {
            match ch { '{' => brace_depth += 1, '}' => brace_depth -= 1, _ => {} }
        }
        if brace_depth != 0 { continue; }
        if trimmed.starts_with("export ") { continue; }
        for prefix in &["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                // Extract the variable name (before `=` or destructuring)
                let name: String = rest.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() {
                    decl_names.push((i, name));
                }
                break;
            }
        }
    }

    // Second pass: for each non-exported decl, check if the name is used
    // elsewhere in the body (not counting the declaration line itself)
    let mut lines_to_strip: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (line_idx, name) in &decl_names {
        let mut used = false;
        for (i, line) in lines.iter().enumerate() {
            if i == *line_idx { continue; }
            if is_identifier_used(name, line) {
                used = true;
                break;
            }
        }
        if !used {
            lines_to_strip.insert(*line_idx);
        }
    }

    // Third pass: rebuild output, stripping `const/let/var NAME = ` from marked lines
    for (i, line) in lines.iter().enumerate() {
        if lines_to_strip.contains(&i) {
            let trimmed = line.trim();
            // Find `= ` and strip everything up to and including it
            if let Some(eq_pos) = trimmed.find(" = ") {
                let after_eq = &trimmed[eq_pos + 3..];
                // Don't strip declarations whose initializer is a named marker
                // QRL call (e.g., `const Header = /*#__PURE__*/ componentQrl(...)`).
                // These were intentionally preserved by strip_non_exported_var_decl
                // because the marker's import line has non-marker specifiers.
                if after_eq.contains("Qrl(") {
                    result.push_str(line);
                    result.push('\n');
                    continue;
                }
                // Preserve leading whitespace from original line
                let leading: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                result.push_str(&leading);
                result.push_str(after_eq);
                result.push('\n');
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }

    // Preserve trailing newline behavior
    if !body.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
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

/// Strip `/* @qwik-disable-next-line ... */` directive comments from parent module body.
fn strip_qwik_directive_comments_parent(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    for line in body.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("/* @qwik-disable") && trimmed.ends_with("*/"))
            || trimmed.starts_with("// @qwik-disable")
        {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    if !body.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

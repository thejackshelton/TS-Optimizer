/// Generates source code for extracted segment modules.
///
/// Injects _captures unpacking, manages imports, assembles segment exports.

use std::collections::BTreeSet;

use oxc_span::GetSpan;

use super::extract::ExtractionResult;
use super::jsx_transform::{transform_jsx_in_segment_with_key, set_signal_context_with_vars, take_signal_hoister};
use super::marker_detection::{ImportInfo, to_qrl_name};
use super::rewrite_calls::{build_qrl_declaration, qrl_var_name};
use super::rewrite_parent::rename_legacy_package;

/// Sort key for segment imports to match SWC output ordering.
/// SWC sorts by imported specifier name (case-sensitive alphabetical),
/// but groups relative imports before framework imports.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ImportSortKey {
    /// Priority bucket: 0 = relative (./ or ../), 1 = bare, 2 = scoped (@)
    priority: u8,
    /// Import specifier name for sorting within each bucket
    name: String,
    /// Source path for tiebreaking
    source: String,
}

/// Extract sort key from an import line string like `import { foo } from "source";`
fn import_sort_key_from_line(line: &str) -> ImportSortKey {
    // Extract source from `from "source"` or `from 'source'`
    let source = if let Some(pos) = line.rfind("from \"") {
        let start = pos + 6;
        let end = line[start..].find('"').map(|e| start + e).unwrap_or(line.len());
        &line[start..end]
    } else if let Some(pos) = line.rfind("from '") {
        let start = pos + 6;
        let end = line[start..].find('\'').map(|e| start + e).unwrap_or(line.len());
        &line[start..end]
    } else {
        line
    };

    // Extract the imported name for secondary sorting
    // `import { Foo } from ...` -> "Foo"
    // `import { Foo as Bar } from ...` -> "Foo" (sort by imported name, not local)
    // `import Foo from ...` -> "Foo"
    // `import * as Foo from ...` -> "* as Foo"
    let name = if let Some(brace_start) = line.find('{') {
        if let Some(brace_end) = line.find('}') {
            let inner = line[brace_start + 1..brace_end].trim();
            // Get the first specifier name (before `as` if renamed)
            inner.split(" as ").next().unwrap_or(inner).trim()
        } else {
            ""
        }
    } else if line.starts_with("import * as ") {
        "* as"
    } else if line.starts_with("import ") {
        // Default import: `import Foo from ...`
        let after_import = line[7..].trim();
        after_import.split_whitespace().next().unwrap_or("")
    } else {
        ""
    };

    import_sort_key(source, name)
}

fn import_sort_key(source: &str, name: &str) -> ImportSortKey {
    // SWC sorts segment imports alphabetically by specifier name,
    // with relative imports (./...) first.
    let priority = if source.starts_with("./") || source.starts_with("../") {
        0  // relative imports first
    } else {
        1  // all non-relative (bare, scoped) share the same bucket
    };
    ImportSortKey {
        priority,
        source: source.to_string(),
        name: name.to_string(),
    }
}

/// Generate the code for a segment module.
///
/// A segment module exports the extracted function with its symbol name.
/// `parent_file_stem` is the file name without extension, e.g., "test" for "test.tsx".
/// `module_var_names` are module-level declarations that should be imported from the parent.
///
/// When `all_extractions` is provided (non-empty), nested `$()` calls within this
/// segment's body are replaced with QRL variable references, and corresponding
/// QRL declarations and imports are emitted.
pub fn generate_segment_code(
    extraction: &ExtractionResult,
    symbol_name: &str,
    imports: &[ImportInfo],
    captures: &[String],
    _nested_qrl_decls: &[String],
    parent_file_stem: &str,
    module_var_names: &[String],
    transpile_jsx: bool,
    transpile_ts: bool,
    // New parameters for nested QRL extraction
    current_index: usize,
    all_extractions: &[ExtractionResult],
    all_symbol_names: &[String],
    all_canonical_filenames: &[String],
    all_captures: &[Vec<String>],
    segment_extensions: &[String],
    explicit_extensions: bool,
    iteration_captures: &[String],
    // Map of destructured prop names to replace with `_rawProps.name` in the body.
    // Non-empty only for child segments whose parent is a component$ with destructured props.
    props_remap: &std::collections::HashSet<String>,
) -> String {
    let mut code = String::new();

    // Start with the raw body text — nested QRL replacement must happen first
    // because call_start/call_end are absolute positions matching the raw text offsets
    let mut body = extraction.body_text.clone();

    // Find child extractions (nested $() calls whose parent is this segment)
    let parent_key = format!("segment_{}", current_index);
    let mut children: Vec<(usize, &ExtractionResult)> = Vec::new();
    for (j, child) in all_extractions.iter().enumerate() {
        if child.parent_segment.as_deref() == Some(&parent_key) {
            children.push((j, child));
        }
    }

    // Replace nested $() calls in the body with QRL variable references
    // Process from end to start to preserve offsets
    let mut qrl_decls: Vec<String> = Vec::new();
    let mut needs_qrl_import = false;

    if !children.is_empty() {
        // Sort children by call_start descending so we replace from end to start
        children.sort_by(|a, b| b.1.call_start.cmp(&a.1.call_start));

        let body_start = extraction.start as usize;

        for (j, child) in &children {
            let child_call_start = child.call_start as usize;
            let child_call_end = child.call_end as usize;

            // Compute positions relative to the segment body
            if child_call_start < body_start || child_call_end < body_start {
                continue; // Safety: skip if positions don't make sense
            }
            let rel_start = child_call_start - body_start;
            let rel_end = child_call_end - body_start;

            if rel_end > body.len() || rel_start > body.len() {
                continue; // Safety: skip if out of bounds
            }

            let child_symbol = &all_symbol_names[*j];
            let child_canonical = &all_canonical_filenames[*j];
            let child_captures = &all_captures[*j];
            let var_name = qrl_var_name(child_symbol);

            // For hook-style $ markers (e.g., useStyles$, useComputed$), wrap with Qrl form:
            // `useStyles$(...)` → `useStylesQrl(q_...)`
            // For bare `$` marker or event handlers (onClick$, etc.), just use the variable name.
            // JSX attribute markers (in_jsx=true, like `custom$={...}`) are NOT wrapped.
            let marker = &child.marker_name;
            let is_event_handler = is_jsx_event_marker(marker);
            let is_prefixed_event = marker.contains(':');
            let is_jsx_prop = child.in_jsx;
            let should_wrap = marker != "$"
                && marker.ends_with('$')
                && !is_event_handler
                && !is_prefixed_event
                && !is_jsx_prop;
            // Append .w([captures]) if the child has captures
            // SWC uses multi-line format with each capture on its own line
            let var_with_captures = if child_captures.is_empty() {
                var_name.clone()
            } else {
                format!("{}.w([\n{}\n])", var_name,
                    child_captures.join(",\n"))
            };
            let replacement = if should_wrap {
                let qrl_fn = to_qrl_name(marker);
                // component$ gets /*#__PURE__*/ annotation, matching SWC behavior
                if marker == "component$" {
                    format!("/*#__PURE__*/ {}({})", qrl_fn, var_with_captures)
                } else {
                    format!("{}({})", qrl_fn, var_with_captures)
                }
            } else {
                var_with_captures
            };

            // Replace the $(...) call in the body with the replacement
            body = format!("{}{}{}", &body[..rel_start], replacement, &body[rel_end..]);

            // Build QRL declaration for this child
            let child_ext = if explicit_extensions {
                segment_extensions.get(*j).map(|s| s.as_str()).unwrap_or("")
            } else { "" };
            let decl = build_qrl_declaration(child_symbol, child_canonical, child_captures, child_ext);
            qrl_decls.push(decl);
            needs_qrl_import = true;
        }

        // Sort qrl_decls alphabetically to match SWC's ordering
        qrl_decls.sort();
    }

    // For component$ with destructured props parameter, always replace the
    // destructured pattern with `_rawProps`. SWC does this for component$ bodies —
    // child segments capture `_rawProps` (the whole props object) instead of
    // individual destructured fields.
    if extraction.has_destructured_props && is_component_marker(&extraction.marker_name) {
        body = replace_destructured_props_with_raw_props(&body);
    }

    // Strip @qwik-disable-next-line directive comments from segment bodies.
    // These are authoring-time directives that SWC strips during compilation.
    body = strip_qwik_directive_comments(&body);

    // Strip TypeScript type annotations when transpiling TS.
    // The body text is raw source, so TS annotations (param types, return types)
    // persist unless explicitly removed.
    if transpile_ts {
        body = strip_ts_annotations(&body);
    }

    // Now apply text transformations after QRL replacement
    // Minify arrow function whitespace to match SWC output:
    // `(x) => expr` → `(x)=>expr`
    // `async (x) => expr` → `async (x)=>expr`
    body = minify_arrow(&body);

    // Normalize spaces around compound assignment operators to match SWC:
    // `x+=y` → `x += y`, `x-=y` → `x -= y`, etc.
    body = normalize_compound_assignment_spaces(&body);

    // Normalize destructuring whitespace: `{x}` → `{ x }`
    body = normalize_destructuring_spaces(&body);

    // Strip parentheses from return expressions: `return (\nexpr\n)` → `return expr`
    body = strip_return_parens(&body);

    // Strip parentheses from arrow expression bodies: `=>(expr)` → `=>expr`
    body = strip_arrow_body_parens(&body);

    // Unwrap single-statement if/else blocks to match SWC's minified output:
    // `if (cond) { stmt } else { stmt }` → `if (cond) stmt;\nelse stmt;`
    body = unwrap_single_stmt_if_else(&body);

    // Only apply JSX transformation when transpile_jsx is enabled
    let jsx_transformed = if transpile_jsx {
        // Set up signal analysis context before JSX transform
        let mut imported_names = BTreeSet::new();
        for imp in imports {
            imported_names.insert(imp.local_name.clone());
            imported_names.insert(imp.specifier.clone());
        }
        // Also add well-known framework globals
        imported_names.insert("isServer".to_string());
        imported_names.insert("isBrowser".to_string());

        // Local names: parent scope (module vars) + segment body declarations
        let mut local_names = BTreeSet::new();
        for name in module_var_names {
            local_names.insert(name.clone());
        }
        // Add local declarations from segment body
        if let Some(body_locals) = collect_scope_bindings_ast(&body) {
            for name in body_locals {
                local_names.insert(name);
            }
        } else {
            for name in collect_local_declarations(&body) {
                local_names.insert(name);
            }
        }
        // Add captures as local names
        for cap in captures {
            local_names.insert(cap.clone());
        }

        // Collect variables known to hold Signal values (from useSignal, useComputed$, etc.)
        let signal_var_names = collect_signal_var_names(&body);

        set_signal_context_with_vars(imported_names, local_names, signal_var_names);

        if let Some(transformed) = transform_jsx_in_segment_with_key(&body, extraction.jsx_key_prefix.as_deref()) {
            body = transformed;
            // After JSX transform, collapse `=>\nWHITESPACE` to `=>` to match SWC.
            // This happens when the original was `() => \n<JSX>` and JSX got replaced.
            body = collapse_arrow_newline(&body);
            true
        } else {
            // Clear signal context even if JSX transform didn't apply
            let _ = take_signal_hoister();
            false
        }
    } else {
        // Even without full JSX transpilation, rename event handler props:
        // onClick$={...} → q-e:click={...}
        body = rename_jsx_event_props(&body);
        false
    };

    // Retrieve hoisted signal functions (_hf0, _hf1, etc.) from JSX transform
    let signal_hoister = take_signal_hoister();

    // Dead code elimination: strip unused declarations from the segment body.
    // SWC performs AST-level DCE, removing:
    //   - Variable declarations whose names are unused
    //   - Function/class declarations whose names are unused
    //   - Empty try/catch blocks
    //   - `if (false) { ... }` branches
    body = eliminate_dead_code_in_body(&body);

    // Collect imports needed by this segment's body
    let mut segment_imports = collect_segment_imports(&body, imports, module_var_names, parent_file_stem);

    // Add synthetic imports for JSX helpers used after transformation
    if jsx_transformed {
        let jsx_helpers = [
            ("_jsxSorted", "@qwik.dev/core"),
            ("_jsxSplit", "@qwik.dev/core"),
            ("_jsxC", "@qwik.dev/core"),
            ("_jsxQ", "@qwik.dev/core"),
            ("_jsxS", "@qwik.dev/core"),
            ("_getVarProps", "@qwik.dev/core"),
            ("_getConstProps", "@qwik.dev/core"),
            ("_wrapProp", "@qwik.dev/core"),
            ("_wrapSignal", "@qwik.dev/core"),
            ("_fnSignal", "@qwik.dev/core"),
            ("_Fragment", "@qwik.dev/core/jsx-runtime"),
            ("Fragment", "@qwik.dev/core/jsx-runtime"),
        ];
        for (helper, source) in &jsx_helpers {
            if is_identifier_used(&body, helper) {
                let line = if *helper == "_Fragment" {
                    format!("import {{ Fragment as _Fragment }} from \"{}\";", source)
                } else {
                    format!("import {{ {} }} from \"{}\";", helper, source)
                };
                // Append — these are framework imports that go after user imports
                segment_imports.push(line);
            }
        }
    }

    // Add qrl import if we have nested QRL declarations
    if needs_qrl_import {
        // Insert qrl import — check if it already exists
        let qrl_import = "import { qrl } from \"@qwik.dev/core\";".to_string();
        if !segment_imports.iter().any(|l| l.contains("qrl") && l.contains("@qwik.dev/core")) {
            segment_imports.push(qrl_import);
        }
    }

    // Add synthetic imports for Qrl-suffixed wrappers (useStylesQrl, useComputedQrl, etc.)
    // These are generated when nested $() calls are replaced with their Qrl wrapper form.
    if !children.is_empty() {
        for (_j, child) in &children {
            let marker = &child.marker_name;
            let is_evt = is_jsx_event_marker(marker);
            let is_pfx = marker.contains(':');
            let is_jsx = child.in_jsx;
            if marker != "$" && marker.ends_with('$') && !is_evt && !is_pfx && !is_jsx {
                let qrl_fn = to_qrl_name(marker);
                if is_identifier_used(&body, &qrl_fn) {
                    // Find the source module for this import
                    let source = imports.iter()
                        .find(|imp| imp.local_name == *marker || imp.specifier == *marker)
                        .map(|imp| rename_legacy_package(&imp.source))
                        .unwrap_or_else(|| "@qwik.dev/core".to_string());
                    let import_line = format!("import {{ {} }} from \"{}\";", qrl_fn, source);
                    if !segment_imports.iter().any(|l| l.contains(&qrl_fn)) {
                        segment_imports.push(import_line);
                    }
                }
            }
        }
    }

    // Pre-compute capture flags needed for both import and injection logic.
    let has_iteration_caps = !iteration_captures.is_empty();
    let has_component_caps = !captures.is_empty();

    // Add _captures import if this segment uses the _captures unpacking pattern.
    // This applies to:
    // - Non-event segments with captures
    // - Event handlers with BOTH iteration captures AND component-level captures
    let needs_captures_import = if extraction.ctx_kind == super::types::SegmentKind::EventHandler {
        // Event handlers need _captures only when they have component-level captures
        // alongside iteration captures (hybrid mode)
        has_iteration_caps && has_component_caps
    } else {
        !captures.is_empty()
    };
    if needs_captures_import {
        let captures_import = "import { _captures } from \"@qwik.dev/core\";".to_string();
        if !segment_imports.iter().any(|l| l.contains("_captures")) {
            segment_imports.push(captures_import);
        }
    }

    // Re-sort all imports (including synthetic ones) to match SWC's alphabetical ordering
    segment_imports.sort_by(|a, b| {
        let key_a = import_sort_key_from_line(a);
        let key_b = import_sort_key_from_line(b);
        key_a.cmp(&key_b)
    });
    segment_imports.dedup();

    // Emit import statements
    if !segment_imports.is_empty() || !qrl_decls.is_empty() {
        for import_line in &segment_imports {
            code.push_str(import_line);
            code.push('\n');
        }
        code.push_str("//\n");
    }

    // Emit QRL declarations for nested segments
    if !qrl_decls.is_empty() {
        for decl in &qrl_decls {
            code.push_str(decl);
            code.push_str(";\n");
        }
        code.push_str("//\n");
    }

    // Emit hoisted signal function declarations (_hf0, _hf0_str, etc.)
    if let Some(ref hoister) = signal_hoister {
        for decl_line in hoister.get_declarations() {
            code.push_str(&decl_line);
            code.push('\n');
        }
    }

    // Expand single-line object literals in function call arguments to multi-line
    // to match SWC's AST reprint formatting:
    // `useStore({ thing: 0 })` → `useStore({\n    thing: 0\n})`
    body = expand_object_literals(&body);

    // Ensure return statements in block bodies end with semicolons.
    // SWC reprints the AST so all statements get semicolons, but we preserve
    // source text which may omit them (e.g., `return <div />` without `;`).
    body = ensure_return_semicolons(&body);

    // Apply props_remap: replace bare references to destructured prop names
    // with `_rawProps.name`. This happens when the parent component$ had destructured
    // props like `({foo}) => { ... }` and this child segment captures `_rawProps`.
    if !props_remap.is_empty() {
        body = remap_destructured_props(&body, props_remap);
    }

    // Inject captures depending on segment type:
    // - Event handlers with iteration vars: hybrid approach
    //   - Iteration vars become positional params: `(_, _1, item)=>{ ... }`
    //   - Component-level captures use _captures: `const cart = _captures[0]; ...`
    // - Event handlers without iteration vars: all captures as positional params
    // - Non-event segments: _captures pattern
    if has_iteration_caps && extraction.ctx_kind == super::types::SegmentKind::EventHandler {
        // Hybrid: iteration vars as arrow params + component captures via _captures
        body = inject_captures_into_arrow(&body, iteration_captures);
        if has_component_caps {
            body = inject_captures_unpacking_into_body(&body, captures);
        }
    } else if has_component_caps {
        if extraction.ctx_kind == super::types::SegmentKind::EventHandler {
            body = inject_captures_into_arrow(&body, captures);
        } else {
            body = inject_captures_unpacking(&body, captures);
        }
    }

    // Export the segment function
    code.push_str(&format!(
        "export const {} = {};\n",
        symbol_name,
        body
    ));

    code
}

/// Inject captured variables as function parameters into an arrow function body.
/// Transforms `()=>body` to `(_, _1, cap1, cap2)=>body`.
/// The first two params `_` and `_1` are placeholders for event and element context.
fn inject_captures_into_arrow(body: &str, captures: &[String]) -> String {
    // Build the parameter list: _, _1, then each capture
    let mut params: Vec<String> = vec!["_".to_string(), "_1".to_string()];
    for cap in captures {
        params.push(cap.clone());
    }
    let param_str = params.join(", ");

    // Find the opening `(` and its matching `)` followed by `=>`
    // Handle: `()=>`, `(x)=>`, `(x, y)=>`
    if let Some(paren_start) = body.find('(') {
        let bytes = body.as_bytes();
        let mut depth = 1;
        let mut j = paren_start + 1;
        while j < bytes.len() && depth > 0 {
            if bytes[j] == b'(' { depth += 1; }
            if bytes[j] == b')' { depth -= 1; }
            j += 1;
        }
        // j now points past the closing `)`
        // Check that it's followed by `=>`
        let after_paren = &body[j..].trim_start();
        if after_paren.starts_with("=>") {
            return format!(
                "{}({}){}",
                &body[..paren_start],
                param_str,
                &body[j..] // from after `)` onwards (`=>body`)
            );
        }
    }

    // Fallback: couldn't find arrow pattern, return unchanged
    body.to_string()
}

/// Inject _captures unpacking for non-event-handler segments.
///
/// Transforms an arrow function like `(props)=>{ body }` or `({foo})=>{ body }`
/// into `()=>{ const props = _captures[0]; body }` (stripping original params
/// and adding _captures unpacking at the top of the body).
///
/// SWC pattern: the segment function has no params, and captured variables
/// are unpacked from the global `_captures` array at the top of the body.
fn inject_captures_unpacking(body: &str, captures: &[String]) -> String {
    // Build the _captures unpacking as a single `const` declaration with comma-separated
    // assignments on one line, matching SWC output:
    // `const C2 = _captures[0], C3 = _captures[1], ...;`
    let assignments: Vec<String> = captures.iter().enumerate()
        .map(|(i, cap)| format!("{} = _captures[{}]", cap, i))
        .collect();
    let unpacking_lines = format!("const {};\n", assignments.join(", "));

    // Find the arrow function pattern: `(params)=>{ body }` or `(params)=>expr`
    if let Some(paren_start) = body.find('(') {
        let bytes = body.as_bytes();
        let mut depth = 1;
        let mut j = paren_start + 1;
        while j < bytes.len() && depth > 0 {
            if bytes[j] == b'(' { depth += 1; }
            if bytes[j] == b')' { depth -= 1; }
            j += 1;
        }
        // j now points past the closing `)`
        let after_paren = body[j..].trim_start();
        if after_paren.starts_with("=>") {
            let arrow_pos = body[j..].find("=>").unwrap() + j;
            let after_arrow = body[arrow_pos + 2..].trim_start();

            if after_arrow.starts_with('{') {
                // Block body: `()=>{ ... }`
                // Find the opening brace and inject _captures after it
                let brace_pos = body[arrow_pos + 2..].find('{').unwrap() + arrow_pos + 2;
                // Find whitespace/newline after opening brace
                let after_brace = &body[brace_pos + 1..];
                let newline_offset = after_brace.find('\n').unwrap_or(0);
                let inject_pos = brace_pos + 1 + newline_offset + 1;

                return format!(
                    "{}(){}{}{}",
                    &body[..paren_start],
                    &body[j..=brace_pos], // =>{ part
                    &body[brace_pos + 1..brace_pos + 1 + newline_offset + 1], // newline after {
                    &format!("{}{}", unpacking_lines, &body[inject_pos..])
                );
            } else {
                // Expression body: `()=>expr` - wrap in block with return
                return format!(
                    "{}()=>{{\n{}return {};\n}}",
                    &body[..paren_start],
                    unpacking_lines,
                    after_arrow.trim_end_matches(';'),
                );
            }
        }
    }

    // Fallback
    body.to_string()
}

/// Inject _captures unpacking into the body of an arrow function that already has params.
/// Used for the hybrid pattern where iteration vars are arrow params and component
/// captures use _captures unpacking.
///
/// For block body: `(_, _1, item)=>{ body }` → `(_, _1, item)=>{ const cart = _captures[0]; body }`
/// For expression body: `(_, _1, item)=>expr` → `(_, _1, item)=>{ const cart = _captures[0]; return expr; }`
fn inject_captures_unpacking_into_body(body: &str, captures: &[String]) -> String {
    if captures.is_empty() {
        return body.to_string();
    }

    // Build compact unpacking: `const cart = _captures[0], results = _captures[1];`
    let unpacking = if captures.len() == 1 {
        format!("\tconst {} = _captures[0];\n", captures[0])
    } else {
        let parts: Vec<String> = captures.iter().enumerate()
            .map(|(i, c)| format!("{} = _captures[{}]", c, i))
            .collect();
        format!("\tconst {};\n", parts.join(", "))
    };

    // Find the arrow `=>`
    if let Some(arrow_pos) = body.find("=>") {
        let after_arrow = body[arrow_pos + 2..].trim_start();

        if after_arrow.starts_with('{') {
            // Block body: inject after opening brace
            let brace_abs = body[arrow_pos + 2..].find('{').unwrap() + arrow_pos + 2;
            let after_brace = &body[brace_abs + 1..];
            let newline_offset = after_brace.find('\n').unwrap_or(0);
            let inject_pos = brace_abs + 1 + newline_offset + 1;

            return format!(
                "{}{}{}\t{}",
                &body[..=brace_abs],
                &body[brace_abs + 1..inject_pos],
                unpacking,
                &body[inject_pos..].trim_start_matches('\t'),
            );
        } else {
            // Expression body: wrap in block
            let expr_start = arrow_pos + 2 + (body[arrow_pos + 2..].len() - after_arrow.len());
            let expr = body[expr_start..].trim_end_matches(';');
            return format!(
                "{}=>{{\n{}\treturn {};\n}}",
                &body[..arrow_pos],
                unpacking,
                expr,
            );
        }
    }

    body.to_string()
}

/// Check if `ident` appears as a word boundary in `body`.
/// Uses a simple check: the character before and after the identifier
/// must not be alphanumeric or underscore.
/// Check if a marker name is a JSX event handler.
/// Matches: `onClick$`, `onDblClick$`, `on-anotherCustom$`, etc.
/// Pattern: starts with "on" followed by uppercase letter OR hyphen.
/// Check if a marker name is a component$ (including local aliases).
/// SWC treats component$ specially: destructured props become `_rawProps`.
fn is_component_marker(marker: &str) -> bool {
    marker == "component$"
}

/// Replace bare references to destructured prop names with `_rawProps.name`.
/// E.g., `foo` → `_rawProps.foo` when `foo` was a destructured prop of the parent component.
/// Only replaces at word boundaries (not inside strings, property accesses, etc.).
fn remap_destructured_props(body: &str, names: &std::collections::HashSet<String>) -> String {
    let mut result = body.to_string();
    for name in names {
        let replacement = format!("_rawProps.{}", name);
        result = replace_identifier_with(&result, name, &replacement);
    }
    result
}

/// Replace all occurrences of an identifier at word boundaries.
/// Checks that the char before is not `.` (to avoid replacing property accesses).
fn replace_identifier_with(source: &str, name: &str, replacement: &str) -> String {
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    if name_len == 0 || source.len() < name_len {
        return source.to_string();
    }

    let bytes = source.as_bytes();
    let mut result = String::with_capacity(source.len());
    let mut i = 0;

    while i < bytes.len() {
        if i + name_len <= bytes.len() && &bytes[i..i + name_len] == name_bytes {
            let before_ok = if i == 0 {
                true
            } else {
                let c = bytes[i - 1];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'$' && c != b'.'
            };
            let after_ok = if i + name_len >= bytes.len() {
                true
            } else {
                let c = bytes[i + name_len];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'$'
            };

            if before_ok && after_ok {
                result.push_str(replacement);
                i += name_len;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Replace a destructured object pattern in the function params with `_rawProps`.
///
/// Handles arrow functions: `({foo, bar}) => { ... }` → `(_rawProps) => { ... }`
/// and `async ({foo}) => { ... }` → `async (_rawProps) => { ... }`
///
/// Finds the first `(` then the matching `{` inside it, finds the matching `}`,
/// then the closing `)`, and replaces the whole parameter list with `(_rawProps)`.
fn replace_destructured_props_with_raw_props(body: &str) -> String {
    let bytes = body.as_bytes();

    // Find the opening paren of the parameter list
    let paren_start = match body.find('(') {
        Some(pos) => pos,
        None => return body.to_string(),
    };

    // Find matching closing paren, tracking nesting
    let mut depth = 1;
    let mut j = paren_start + 1;
    while j < bytes.len() && depth > 0 {
        if bytes[j] == b'(' { depth += 1; }
        if bytes[j] == b')' { depth -= 1; }
        j += 1;
    }
    let paren_end = j; // one past the closing `)`

    // Verify this is followed by `=>`
    let after_paren = body[paren_end..].trim_start();
    if !after_paren.starts_with("=>") {
        return body.to_string();
    }

    // Replace the entire param section `(...)` with `(_rawProps)`
    format!("{}(_rawProps){}", &body[..paren_start], &body[paren_end..])
}

fn is_jsx_event_marker(marker: &str) -> bool {
    if !marker.starts_with("on") || marker.len() <= 3 {
        return false;
    }
    let third = marker.chars().nth(2).unwrap_or(' ');
    third.is_uppercase() || third == '-'
}

fn is_identifier_used(body: &str, ident: &str) -> bool {
    let ident_bytes = ident.as_bytes();
    let body_bytes = body.as_bytes();
    let ident_len = ident_bytes.len();

    if ident_len == 0 || body_bytes.len() < ident_len {
        return false;
    }

    let mut i = 0;
    while i + ident_len <= body_bytes.len() {
        if &body_bytes[i..i + ident_len] == ident_bytes {
            // Check word boundary before
            let before_ok = if i == 0 {
                true
            } else {
                let c = body_bytes[i - 1];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'$'
            };
            // Check word boundary after
            let after_ok = if i + ident_len >= body_bytes.len() {
                true
            } else {
                let c = body_bytes[i + ident_len];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'$'
            };
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Collect variable names that are assigned from useSignal() or useComputed$() calls.
/// These are known to hold Signal values and should be classified as constProps in JSX.
/// Matches patterns like: `const x = useSignal(...)`, `let x = useComputed$(...)`.
fn collect_signal_var_names(body: &str) -> BTreeSet<String> {
    let mut signal_vars = BTreeSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        for prefix in &["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                // Extract variable name
                if let Some(name) = rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next() {
                    if !name.is_empty() && name != "{" && name != "[" {
                        // Check if the RHS is a useSignal/useComputed$ call
                        if let Some(eq_pos) = rest.find('=') {
                            let rhs = rest[eq_pos + 1..].trim_start();
                            if rhs.starts_with("useSignal(")
                                || rhs.starts_with("useSignal$(")
                                || rhs.starts_with("useComputed(")
                                || rhs.starts_with("useComputed$(")
                            {
                                signal_vars.insert(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    signal_vars
}

/// Collect variable names declared locally in the segment body.
/// These should NOT be imported from the parent even if they share names.
pub fn collect_local_declarations(body: &str) -> std::collections::HashSet<String> {
    let mut locals = std::collections::HashSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        for prefix in &["const ", "let ", "var "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some(name) = rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next() {
                    if !name.is_empty() && name != "{" && name != "[" {
                        locals.insert(name.to_string());
                    }
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("function ") {
            if let Some(name) = rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next() {
                if !name.is_empty() {
                    locals.insert(name.to_string());
                }
            }
        }
        if let Some(catch_pos) = trimmed.find("catch") {
            let after = &trimmed[catch_pos + 5..];
            let after = after.trim_start();
            if let Some(rest) = after.strip_prefix('(') {
                if let Some(name) = rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next() {
                    if !name.is_empty() {
                        locals.insert(name.to_string());
                    }
                }
            }
        }
    }
    locals
}

/// AST-based extraction of local declarations and function parameters.
/// Wraps the body in `const __fn = <body>` and parses to find all bindings.
/// AST-based extraction of all bindings in a function body: params + local declarations.
/// Used by capture analysis to find parent scope variables.
pub fn collect_scope_bindings_ast(body: &str) -> Option<std::collections::HashSet<String>> {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let wrapped = format!("const __qwik_fn = {};", body);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked {
        return None;
    }

    let mut locals = std::collections::HashSet::new();

    // Find the outermost function/arrow expression
    for stmt in &parse_result.program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let Some(ref init) = declarator.init {
                    collect_fn_bindings(init, &mut locals);
                }
            }
        }
    }

    Some(locals)
}

/// Like `collect_scope_bindings_ast` but recursively collects ALL bindings
/// including nested arrow/function params (e.g., `.map((item, index) => ...)`).
/// Used by capture analysis to find variables from intermediate scopes.
pub fn collect_scope_bindings_deep(body: &str) -> Option<std::collections::HashSet<String>> {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let wrapped = format!("const __qwik_fn = {};", body);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked {
        return None;
    }

    let mut locals = std::collections::HashSet::new();

    for stmt in &parse_result.program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let Some(ref init) = declarator.init {
                    collect_fn_bindings_deep(init, &mut locals);
                }
            }
        }
    }

    Some(locals)
}

/// Like `collect_fn_bindings` but recurses into nested arrow/function
/// expressions to also collect their parameter names.
fn collect_fn_bindings_deep(expr: &oxc_ast::ast::Expression, names: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            for param in &arrow.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            for stmt in &arrow.body.statements {
                collect_stmt_declarations_deep(stmt, names);
            }
        }
        Expression::FunctionExpression(fn_expr) => {
            for param in &fn_expr.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            if let Some(ref body) = fn_expr.body {
                for stmt in &body.statements {
                    collect_stmt_declarations_deep(stmt, names);
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_fn_bindings_deep(&paren.expression, names);
        }
        _ => {}
    }
}

/// Extract parameter names and local declarations from a function/arrow expression.
/// Only collects the top-level scope (params + direct body declarations).
fn collect_fn_bindings(expr: &oxc_ast::ast::Expression, names: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            // Collect parameter names
            for param in &arrow.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            // Collect local declarations in the body (shallow)
            for stmt in &arrow.body.statements {
                collect_stmt_declarations(stmt, names);
            }
        }
        Expression::FunctionExpression(fn_expr) => {
            for param in &fn_expr.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            if let Some(ref body) = fn_expr.body {
                for stmt in &body.statements {
                    collect_stmt_declarations(stmt, names);
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_fn_bindings(&paren.expression, names);
        }
        _ => {}
    }
}

/// Recursively collect all bindings from an expression, including nested
/// arrow/function parameters. This is needed so that captures analysis can
/// find variables like `item` and `index` from `.map((item, index) => ...)`
/// callbacks that contain event handler extractions.
fn collect_expr_bindings_deep(expr: &oxc_ast::ast::Expression, names: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            for param in &arrow.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            for stmt in &arrow.body.statements {
                collect_stmt_declarations_deep(stmt, names);
            }
        }
        Expression::FunctionExpression(fn_expr) => {
            for param in &fn_expr.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            if let Some(ref body) = fn_expr.body {
                for stmt in &body.statements {
                    collect_stmt_declarations_deep(stmt, names);
                }
            }
        }
        Expression::CallExpression(call) => {
            collect_expr_bindings_deep(&call.callee, names);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_expr_bindings_deep(e, names);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_expr_bindings_deep(&member.object, names);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_expr_bindings_deep(&member.object, names);
            collect_expr_bindings_deep(&member.expression, names);
        }
        Expression::ConditionalExpression(cond) => {
            collect_expr_bindings_deep(&cond.test, names);
            collect_expr_bindings_deep(&cond.consequent, names);
            collect_expr_bindings_deep(&cond.alternate, names);
        }
        Expression::LogicalExpression(logical) => {
            collect_expr_bindings_deep(&logical.left, names);
            collect_expr_bindings_deep(&logical.right, names);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_expr_bindings_deep(&paren.expression, names);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_expr_bindings_deep(e, names);
            }
        }
        Expression::TemplateLiteral(tmpl) => {
            for e in &tmpl.expressions {
                collect_expr_bindings_deep(e, names);
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let ArrayExpressionElement::SpreadElement(spread) = elem {
                    collect_expr_bindings_deep(&spread.argument, names);
                } else if let Some(e) = elem.as_expression() {
                    collect_expr_bindings_deep(e, names);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_expr_bindings_deep(&p.value, names);
                } else if let ObjectPropertyKind::SpreadProperty(spread) = prop {
                    collect_expr_bindings_deep(&spread.argument, names);
                }
            }
        }
        Expression::AssignmentExpression(assign) => {
            collect_expr_bindings_deep(&assign.right, names);
        }
        Expression::BinaryExpression(bin) => {
            collect_expr_bindings_deep(&bin.left, names);
            collect_expr_bindings_deep(&bin.right, names);
        }
        Expression::UnaryExpression(unary) => {
            collect_expr_bindings_deep(&unary.argument, names);
        }
        Expression::UpdateExpression(_) => {
            // UpdateExpression argument is a SimpleAssignmentTarget, not Expression
        }
        Expression::AwaitExpression(a) => {
            collect_expr_bindings_deep(&a.argument, names);
        }
        Expression::YieldExpression(y) => {
            if let Some(ref arg) = y.argument {
                collect_expr_bindings_deep(arg, names);
            }
        }
        _ => {}
    }
}

/// Like `collect_stmt_declarations` but also recurses into expressions
/// to find nested arrow/function parameter names.
fn collect_stmt_declarations_deep(stmt: &oxc_ast::ast::Statement, names: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match stmt {
        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                collect_binding_pattern_names(&declarator.id, names);
                if let Some(ref init) = declarator.init {
                    collect_expr_bindings_deep(init, names);
                }
            }
        }
        Statement::FunctionDeclaration(fn_decl) => {
            if let Some(ref id) = fn_decl.id {
                names.insert(id.name.to_string());
            }
            for param in &fn_decl.params.items {
                collect_binding_pattern_names(&param.pattern, names);
            }
            if let Some(ref body) = fn_decl.body {
                for s in &body.statements {
                    collect_stmt_declarations_deep(s, names);
                }
            }
        }
        Statement::ClassDeclaration(cls) => {
            if let Some(ref id) = cls.id {
                names.insert(id.name.to_string());
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_stmt_declarations_deep(s, names);
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            collect_expr_bindings_deep(&expr_stmt.expression, names);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                collect_expr_bindings_deep(arg, names);
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_expr_bindings_deep(&if_stmt.test, names);
            collect_stmt_declarations_deep(&if_stmt.consequent, names);
            if let Some(ref alt) = if_stmt.alternate {
                collect_stmt_declarations_deep(alt, names);
            }
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(ForStatementInit::VariableDeclaration(ref decl)) = for_stmt.init {
                for declarator in &decl.declarations {
                    collect_binding_pattern_names(&declarator.id, names);
                }
            }
            collect_stmt_declarations_deep(&for_stmt.body, names);
        }
        Statement::ForInStatement(for_in) => {
            if let ForStatementLeft::VariableDeclaration(ref decl) = for_in.left {
                for declarator in &decl.declarations {
                    collect_binding_pattern_names(&declarator.id, names);
                }
            }
            collect_stmt_declarations_deep(&for_in.body, names);
        }
        Statement::ForOfStatement(for_of) => {
            if let ForStatementLeft::VariableDeclaration(ref decl) = for_of.left {
                for declarator in &decl.declarations {
                    collect_binding_pattern_names(&declarator.id, names);
                }
            }
            collect_stmt_declarations_deep(&for_of.body, names);
        }
        Statement::WhileStatement(while_stmt) => {
            collect_stmt_declarations_deep(&while_stmt.body, names);
        }
        Statement::DoWhileStatement(do_while) => {
            collect_stmt_declarations_deep(&do_while.body, names);
        }
        Statement::TryStatement(try_stmt) => {
            for s in &try_stmt.block.body {
                collect_stmt_declarations_deep(s, names);
            }
            if let Some(ref catch) = try_stmt.handler {
                if let Some(ref param) = catch.param {
                    collect_binding_pattern_names(&param.pattern, names);
                }
                for s in &catch.body.body {
                    collect_stmt_declarations_deep(s, names);
                }
            }
        }
        Statement::SwitchStatement(switch) => {
            for case in &switch.cases {
                for s in &case.consequent {
                    collect_stmt_declarations_deep(s, names);
                }
            }
        }
        _ => {}
    }
}

/// Extract names from a binding pattern (handles destructuring).
fn collect_binding_pattern_names(pattern: &oxc_ast::ast::BindingPattern, names: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(id) => {
            let name = id.name.to_string();
            if name != "_rawProps" { // Don't treat _rawProps as a local name
                names.insert(name);
            }
        }
        BindingPatternKind::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_pattern_names(&prop.value, names);
            }
            if let Some(ref rest) = obj.rest {
                collect_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPatternKind::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_pattern_names(elem, names);
            }
            if let Some(ref rest) = arr.rest {
                collect_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPatternKind::AssignmentPattern(assign) => {
            collect_binding_pattern_names(&assign.left, names);
        }
    }
}

/// Collect local variable/function declarations from a statement.
fn collect_stmt_declarations(stmt: &oxc_ast::ast::Statement, names: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match stmt {
        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                collect_binding_pattern_names(&declarator.id, names);
            }
        }
        Statement::FunctionDeclaration(fn_decl) => {
            if let Some(ref id) = fn_decl.id {
                names.insert(id.name.to_string());
            }
        }
        Statement::ClassDeclaration(cls) => {
            if let Some(ref id) = cls.id {
                names.insert(id.name.to_string());
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_stmt_declarations(s, names);
            }
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(ForStatementInit::VariableDeclaration(ref decl)) = for_stmt.init {
                for declarator in &decl.declarations {
                    collect_binding_pattern_names(&declarator.id, names);
                }
            }
            collect_stmt_declarations(&for_stmt.body, names);
        }
        Statement::ForInStatement(for_in) => {
            if let ForStatementLeft::VariableDeclaration(ref decl) = for_in.left {
                for declarator in &decl.declarations {
                    collect_binding_pattern_names(&declarator.id, names);
                }
            }
            collect_stmt_declarations(&for_in.body, names);
        }
        Statement::ForOfStatement(for_of) => {
            if let ForStatementLeft::VariableDeclaration(ref decl) = for_of.left {
                for declarator in &decl.declarations {
                    collect_binding_pattern_names(&declarator.id, names);
                }
            }
            collect_stmt_declarations(&for_of.body, names);
        }
        Statement::TryStatement(try_stmt) => {
            for s in &try_stmt.block.body {
                collect_stmt_declarations(s, names);
            }
            if let Some(ref catch) = try_stmt.handler {
                if let Some(ref param) = catch.param {
                    collect_binding_pattern_names(&param.pattern, names);
                }
                for s in &catch.body.body {
                    collect_stmt_declarations(s, names);
                }
            }
        }
        _ => {}
    }
}

/// Collect all import statements needed by a segment body.
///
/// Returns a sorted list of import statement strings.
/// - Re-emits parent module imports used in the body (from `imports`)
/// - Imports module-level variables from the parent file (from `module_var_names`)
fn collect_segment_imports(
    body: &str,
    imports: &[ImportInfo],
    module_var_names: &[String],
    parent_file_stem: &str,
) -> Vec<String> {
    // Collect locally declared variables to exclude from imports
    let local_decls = collect_local_declarations(body);

    // Collect imports as (sort_key, line) tuples for SWC-compatible ordering.
    // SWC orders: module-local refs first, then other relative, then bare, then scoped.
    let mut import_entries: Vec<(ImportSortKey, String)> = Vec::new();

    // Check parent module imports used in the segment body
    for imp in imports {
        // Skip type-only imports
        if imp.is_type {
            continue;
        }
        // Skip if locally declared (shadowed)
        if local_decls.contains(&imp.local_name) {
            continue;
        }
        if is_identifier_used(body, &imp.local_name) {
            let source = rename_legacy_package(&imp.source);
            let line = if imp.specifier == "default" {
                format!("import {} from \"{}\";", imp.local_name, source)
            } else if imp.specifier == "*" {
                format!("import * as {} from \"{}\";", imp.local_name, source)
            } else if imp.specifier == imp.local_name {
                format!("import {{ {} }} from \"{}\";", imp.local_name, source)
            } else {
                format!("import {{ {} as {} }} from \"{}\";", imp.specifier, imp.local_name, source)
            };
            let key = import_sort_key(&source, &imp.local_name);
            import_entries.push((key, line));
        }
    }

    // Check module-level variables used in the segment body
    for var_name in module_var_names {
        // Skip if locally declared (shadowed)
        if local_decls.contains(var_name) {
            continue;
        }
        if is_identifier_used(body, var_name) {
            let source = format!("./{}", parent_file_stem);
            let line = format!("import {{ {} }} from \"{}\";", var_name, source);
            let key = import_sort_key(&source, var_name);
            import_entries.push((key, line));
        }
    }

    // Sort by key and deduplicate
    import_entries.sort_by(|a, b| a.0.cmp(&b.0));
    import_entries.dedup_by(|a, b| a.1 == b.1);
    import_entries.into_iter().map(|(_, line)| line).collect()
}

/// Strip parentheses from `return (expr);` → `return expr;`.
fn strip_return_parens(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if (trimmed == "return (" || trimmed == "return(") && i + 1 < lines.len() {
            let mut depth = 1;
            let mut end = i + 1;
            while end < lines.len() && depth > 0 {
                for ch in lines[end].trim().chars() {
                    match ch { '(' => depth += 1, ')' => depth -= 1, _ => {} }
                }
                if depth > 0 { end += 1; }
            }
            if depth == 0 {
                let close = lines[end].trim();
                if close == ");" || close == ")" {
                    let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
                    result.push_str(indent);
                    result.push_str("return ");
                    result.push_str(lines[i + 1].trim());
                    result.push('\n');
                    for j in (i + 2)..end {
                        result.push_str(lines[j]);
                        result.push('\n');
                    }
                    // Add semicolon to the last content line (both `)` and `);` cases)
                    if result.ends_with('\n') { result.pop(); }
                    let last_char = result.chars().last().unwrap_or(' ');
                    if last_char != ';' {
                        result.push(';');
                    }
                    result.push('\n');
                    i = end + 1;
                    continue;
                }
            }
        }
        result.push_str(lines[i]);
        result.push('\n');
        i += 1;
    }
    if !body.ends_with('\n') && result.ends_with('\n') { result.pop(); }
    result
}

/// Strip parentheses from arrow expression bodies.
/// `()=>(expr)` → `()=>expr`
/// `()=>(assignment = value)` → `()=>assignment = value`
/// Only strips when the body is a single parenthesized expression (not a block body).
fn strip_arrow_body_parens(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Look for `=>(` or `=>\n(` (arrow followed by optional whitespace then paren)
        if i + 2 < len && bytes[i] == b'=' && bytes[i + 1] == b'>' {
            // Skip whitespace/newlines after `=>`
            let mut ws_end = i + 2;
            while ws_end < len && (bytes[ws_end] == b' ' || bytes[ws_end] == b'\n' || bytes[ws_end] == b'\r' || bytes[ws_end] == b'\t') {
                ws_end += 1;
            }
            if ws_end < len && bytes[ws_end] == b'(' {
                // Verify this is an arrow (preceded by `)` possibly with whitespace)
                let mut k = i.wrapping_sub(1);
                if i > 0 {
                    k = i - 1;
                    while k > 0 && bytes[k] == b' ' { k -= 1; }
                }
                let is_arrow = i == 0 || bytes[k] == b')';

                if is_arrow {
                // Find matching closing paren
                let paren_start = ws_end;
                let mut depth = 1;
                let mut j = paren_start + 1;
                let mut in_sq = false;
                let mut in_dq = false;
                let mut in_tpl = false;
                while j < len && depth > 0 {
                    if !in_dq && !in_tpl && bytes[j] == b'\'' && (j == 0 || bytes[j-1] != b'\\') { in_sq = !in_sq; }
                    else if !in_sq && !in_tpl && bytes[j] == b'"' && (j == 0 || bytes[j-1] != b'\\') { in_dq = !in_dq; }
                    else if !in_sq && !in_dq && bytes[j] == b'`' && (j == 0 || bytes[j-1] != b'\\') { in_tpl = !in_tpl; }
                    if !in_sq && !in_dq && !in_tpl {
                        if bytes[j] == b'(' { depth += 1; }
                        if bytes[j] == b')' { depth -= 1; }
                    }
                    j += 1;
                }
                if depth == 0 {
                    // j points past the closing paren
                    // Check that what follows is `;` or end-of-line/whitespace (not another expression)
                    let after = &body[j..].trim_start();
                    let is_end = after.is_empty() || after.starts_with(';') || after.starts_with('\n');
                    let inner = &body[paren_start + 1..j - 1];
                    let has_newlines = inner.contains('\n');

                    if is_end && !has_newlines {
                        // Single-line: strip the parens: emit `=>inner` instead of `=>(inner)`
                        result.push('=');
                        result.push('>');
                        result.push_str(inner);
                        i = j;
                        continue;
                    }
                    // Multi-line: strip parens and trim inner whitespace
                    // `=>(\nexpr\n)` → `=>expr`
                    if is_end && has_newlines {
                        let trimmed = inner.trim();
                        // Only strip if the trimmed content is a single expression
                        // (no semicolons except at end, no top-level commas)
                        let no_semicolons = !trimmed.contains(';') || trimmed.ends_with(';');
                        if no_semicolons && !trimmed.is_empty() {
                            result.push('=');
                            result.push('>');
                            result.push_str(trimmed);
                            i = j;
                            continue;
                        }
                    }
                }
            }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// Unwrap single-statement if/else blocks to match SWC's minified output.
/// Handles both multi-line and single-line patterns:
/// - `if (cond) {\n  stmt\n} else {\n  stmt2\n}` → `if (cond) stmt;\nelse stmt2;`
/// - `if (cond) { stmt } else { stmt2 }` → `if (cond) stmt;\nelse stmt2;`
/// Only applies when both if and else bodies contain exactly one statement.
fn unwrap_single_stmt_if_else(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Match: `if (...) {`
        if trimmed.starts_with("if ") && trimmed.ends_with('{') && !trimmed.contains('}') {
            let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
            let if_cond = &trimmed[..trimmed.len() - 1].trim_end(); // strip trailing `{`

            // Check for single statement in if-body: next line is stmt, line after is `} else {`
            if i + 2 < lines.len() {
                let stmt1 = lines[i + 1].trim();
                let close_else = lines[i + 2].trim();

                if close_else == "} else {" && i + 4 < lines.len() {
                    let stmt2 = lines[i + 3].trim();
                    let close = lines[i + 4].trim();

                    if close == "}" {
                        // Single statements in both branches — unwrap
                        let s1 = if stmt1.ends_with(';') { stmt1.to_string() } else { format!("{};", stmt1) };
                        let s2 = if stmt2.ends_with(';') { stmt2.to_string() } else { format!("{};", stmt2) };
                        result.push_str(indent);
                        result.push_str(&format!("{} {}\n", if_cond, s1));
                        result.push_str(indent);
                        result.push_str(&format!("else {}\n", s2));
                        i += 5;
                        continue;
                    }
                }
            }
        }

        // Single-line pattern: `if (cond) { stmt } else { stmt2 }`
        if trimmed.starts_with("if ") && trimmed.contains("} else {") {
            if let Some(result_line) = try_unwrap_single_line_if_else(trimmed) {
                let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
                result.push_str(indent);
                result.push_str(&result_line);
                result.push('\n');
                i += 1;
                continue;
            }
        }

        result.push_str(lines[i]);
        result.push('\n');
        i += 1;
    }
    if !body.ends_with('\n') && result.ends_with('\n') { result.pop(); }
    result
}

/// Try to unwrap a single-line `if (cond) { stmt } else { stmt2 }` pattern.
/// Returns the unwrapped form: `if (cond) stmt;\nelse stmt2;`
fn try_unwrap_single_line_if_else(line: &str) -> Option<String> {
    // Find the condition: `if (...)` — need to find matching paren
    let paren_start = line.find('(')?;
    let bytes = line.as_bytes();
    let mut depth = 1;
    let mut j = paren_start + 1;
    while j < bytes.len() && depth > 0 {
        if bytes[j] == b'(' { depth += 1; }
        if bytes[j] == b')' { depth -= 1; }
        j += 1;
    }
    if depth != 0 { return None; }
    // j points past closing paren of condition
    let cond_end = j;
    let after_cond = line[cond_end..].trim_start();

    // Expect `{ stmt } else { stmt2 }`
    if !after_cond.starts_with('{') { return None; }

    // Find the `} else {` boundary
    let else_pos = after_cond.find("} else {")?;
    let if_body = after_cond[1..else_pos].trim();
    let else_rest = &after_cond[else_pos + 8..]; // after `} else {`

    // The else body should end with `}`
    let else_body = else_rest.trim();
    if !else_body.ends_with('}') { return None; }
    let else_stmt = else_body[..else_body.len() - 1].trim();

    // Only unwrap if both are single statements (no nested braces)
    if if_body.contains('{') || else_stmt.contains('{') { return None; }

    let if_cond = &line[..cond_end];
    let s1 = if if_body.ends_with(';') { if_body.to_string() } else { format!("{};", if_body) };
    let s2 = if else_stmt.ends_with(';') { else_stmt.to_string() } else { format!("{};", else_stmt) };

    Some(format!("{} {}\nelse {}", if_cond.trim_end(), s1, s2))
}

/// Rename JSX event handler props in non-transpiled JSX.
/// `onClick$={...}` → `q-e:click={...}`
/// `onDblClick$={...}` → `q-e:dblclick={...}`
/// `document:onFocus$={...}` → `q-d:focus={...}`
/// `window:onClick$={...}` → `q-w:click={...}`
fn rename_jsx_event_props(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Check for optional prefix: `document:on` or `window:on`
        let (prefix, qwik_prefix, on_start) = if i + 11 < len && &body[i..i + 11] == "document:on" {
            ("document:", "q-d:", i + 11)
        } else if i + 9 < len && &body[i..i + 9] == "window:on" {
            ("window:", "q-w:", i + 9)
        } else if i + 2 < len && &body[i..i + 2] == "on" && i + 2 < len && bytes[i + 2].is_ascii_uppercase() {
            // Check we're in a JSX attribute context: preceded by space or newline
            let before_ok = i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\n' || bytes[i - 1] == b'\t';
            if before_ok {
                ("", "q-e:", i + 2)
            } else {
                result.push(bytes[i] as char);
                i += 1;
                continue;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
            continue;
        };

        // Now at on_start, we expect an uppercase letter followed by alphanumeric, then `$=`
        if on_start >= len || !bytes[on_start].is_ascii_uppercase() {
            result.push_str(prefix);
            result.push_str("on");
            i = on_start;
            continue;
        }

        // Read the event name (e.g., "Click", "DblClick", "Focus")
        let mut name_end = on_start;
        while name_end < len && bytes[name_end].is_ascii_alphanumeric() {
            name_end += 1;
        }

        // Check for `$=` or `$={` after the name
        if name_end + 1 < len && bytes[name_end] == b'$' && bytes[name_end + 1] == b'=' {
            let event_name: String = body[on_start..name_end].chars()
                .map(|c| c.to_lowercase().next().unwrap_or(c))
                .collect();
            result.push_str(qwik_prefix);
            result.push_str(&event_name);
            result.push('=');
            i = name_end + 2; // skip past `$=`
        } else {
            // Not an event handler prop, emit original text
            if !prefix.is_empty() {
                result.push_str(prefix);
                result.push_str("on");
            } else {
                result.push_str("on");
            }
            i = on_start;
        }
    }

    result
}

/// Expand single-line object literals in function call arguments to multi-line format.
/// SWC reprints `useStore({ thing: 0 })` as `useStore({\n    thing: 0\n})`.
/// This handles the common case of a function call with a single object literal argument.
fn expand_object_literals(body: &str) -> String {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::Parser;
    use oxc_span::{GetSpan, SourceType};

    // Wrap in a function assignment to make it parseable
    let wrapper_prefix = "const __qwik_fn = ";
    let wrapped = format!("{}{};", wrapper_prefix, body);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked || !parse_result.errors.is_empty() {
        return body.to_string();
    }

    // Collect object literal spans that need expansion
    // We look for ObjectExpression nodes that are on a single line
    let mut expansions: Vec<(usize, usize, String)> = Vec::new(); // (start, end, replacement)
    let prefix_len = wrapper_prefix.len();

    fn visit_expr(
        expr: &Expression,
        wrapped: &str,
        prefix_len: usize,
        body: &str,
        expansions: &mut Vec<(usize, usize, String)>,
        indent_level: usize,
    ) {
        match expr {
            Expression::CallExpression(call) => {
                // Visit callee
                visit_expr(&call.callee, wrapped, prefix_len, body, expansions, indent_level);
                // Check each argument
                for arg in &call.arguments {
                    match arg {
                        Argument::ObjectExpression(obj) => {
                            try_expand_object(obj, wrapped, prefix_len, body, expansions, indent_level);
                        }
                        _ => {
                            if let Some(expr) = arg.as_expression() {
                                visit_expr(expr, wrapped, prefix_len, body, expansions, indent_level);
                            }
                        }
                    }
                }
            }
            Expression::ArrayExpression(arr) => {
                try_expand_array(arr, wrapped, prefix_len, body, expansions, indent_level);
            }
            Expression::ArrowFunctionExpression(arrow) => {
                for stmt in &arrow.body.statements {
                    visit_stmt(stmt, wrapped, prefix_len, body, expansions, indent_level);
                }
            }
            Expression::AssignmentExpression(assign) => {
                visit_expr(&assign.right, wrapped, prefix_len, body, expansions, indent_level);
            }
            _ => {}
        }
    }

    fn visit_stmt(
        stmt: &Statement,
        wrapped: &str,
        prefix_len: usize,
        body: &str,
        expansions: &mut Vec<(usize, usize, String)>,
        indent_level: usize,
    ) {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    if let Some(ref init) = d.init {
                        visit_expr(init, wrapped, prefix_len, body, expansions, indent_level);
                    }
                }
            }
            Statement::ExpressionStatement(es) => {
                visit_expr(&es.expression, wrapped, prefix_len, body, expansions, indent_level);
            }
            Statement::ReturnStatement(ret) => {
                if let Some(ref arg) = ret.argument {
                    // Handle return with object literal: `return { ... }`
                    if let Expression::ObjectExpression(obj) = arg {
                        try_expand_object(obj, wrapped, prefix_len, body, expansions, indent_level);
                    } else {
                        visit_expr(arg, wrapped, prefix_len, body, expansions, indent_level);
                    }
                }
            }
            _ => {}
        }
    }

    fn try_expand_object(
        obj: &ObjectExpression,
        _wrapped: &str,
        prefix_len: usize,
        body: &str,
        expansions: &mut Vec<(usize, usize, String)>,
        indent_level: usize,
    ) {
        let span = obj.span();
        let start = span.start as usize;
        let end = span.end as usize;

        // Only expand if it's on a single line in the source
        if start < prefix_len || end > prefix_len + body.len() {
            return;
        }
        let body_start = start - prefix_len;
        let body_end = end - prefix_len;
        let obj_text = &body[body_start..body_end];

        // Skip if already multi-line
        if obj_text.contains('\n') {
            return;
        }

        // Skip empty objects
        if obj.properties.is_empty() {
            return;
        }

        // Skip objects with nested objects/arrays (too complex for simple expansion)
        // Only expand simple `{ key: value, key2: value2 }` patterns
        let inner = obj_text.trim_start_matches('{').trim_end_matches('}').trim();
        if inner.is_empty() {
            return;
        }

        // Determine the indentation of the line containing this object
        let line_indent = {
            let before = &body[..body_start];
            let last_newline = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_start = &before[last_newline..];
            let indent_count = line_start.len() - line_start.trim_start().len();
            indent_count
        };

        // Build multi-line version with SWC-style indentation
        let base_indent = " ".repeat(line_indent + indent_level * 4);
        let prop_indent = format!("{}    ", base_indent);

        // Split by comma, handling nested structures
        let props = split_object_props(inner);
        if props.is_empty() {
            return;
        }

        let mut expanded = String::from("{\n");
        for (i, prop) in props.iter().enumerate() {
            expanded.push_str(&prop_indent);
            expanded.push_str(prop.trim());
            if i < props.len() - 1 {
                expanded.push(',');
            }
            expanded.push('\n');
        }
        expanded.push_str(&base_indent);
        expanded.push('}');

        expansions.push((body_start, body_end, expanded));
    }

    fn try_expand_array(
        arr: &ArrayExpression,
        _wrapped: &str,
        prefix_len: usize,
        body: &str,
        expansions: &mut Vec<(usize, usize, String)>,
        indent_level: usize,
    ) {
        let span = arr.span();
        let start = span.start as usize;
        let end = span.end as usize;

        if start < prefix_len || end > prefix_len + body.len() {
            return;
        }
        let body_start = start - prefix_len;
        let body_end = end - prefix_len;
        let arr_text = &body[body_start..body_end];

        // Skip if already multi-line
        if arr_text.contains('\n') {
            return;
        }

        // Skip empty arrays or single-element arrays
        if arr.elements.len() <= 1 {
            return;
        }

        let inner = arr_text.trim_start_matches('[').trim_end_matches(']').trim();
        if inner.is_empty() {
            return;
        }

        // Determine indentation
        let line_indent = {
            let before = &body[..body_start];
            let last_newline = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_start = &before[last_newline..];
            line_start.len() - line_start.trim_start().len()
        };

        let base_indent = " ".repeat(line_indent + indent_level * 4);
        let elem_indent = format!("{}    ", base_indent);

        let elems = split_object_props(inner);
        if elems.is_empty() {
            return;
        }

        let mut expanded = String::from("[\n");
        for (i, elem) in elems.iter().enumerate() {
            expanded.push_str(&elem_indent);
            expanded.push_str(elem.trim());
            if i < elems.len() - 1 {
                expanded.push(',');
            }
            expanded.push('\n');
        }
        expanded.push_str(&base_indent);
        expanded.push(']');

        expansions.push((body_start, body_end, expanded));
    }

    // Start the traversal from the top-level statements
    let program = &parse_result.program;
    for stmt in &program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for d in &decl.declarations {
                if let Some(ref init) = d.init {
                    visit_expr(init, &wrapped, prefix_len, body, &mut expansions, 0);
                }
            }
        }
    }

    if expansions.is_empty() {
        return body.to_string();
    }

    // Sort by start position descending to apply replacements without offset shifts
    expansions.sort_by(|a, b| b.0.cmp(&a.0));

    let mut result = body.to_string();
    for (start, end, replacement) in &expansions {
        if *end <= result.len() {
            result = format!("{}{}{}", &result[..*start], replacement, &result[*end..]);
        }
    }

    result
}

/// Split object properties by commas, respecting nesting depth.
fn split_object_props(s: &str) -> Vec<String> {
    let mut props = Vec::new();
    let mut current = String::new();
    let mut depth = 0; // nesting depth for {}, [], ()
    let bytes = s.as_bytes();
    let mut in_string = false;
    let mut string_char = b'"';

    for i in 0..bytes.len() {
        let c = bytes[i];
        if in_string {
            current.push(c as char);
            if c == string_char && (i == 0 || bytes[i - 1] != b'\\') {
                in_string = false;
            }
            continue;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            in_string = true;
            string_char = c;
            current.push(c as char);
            continue;
        }
        if matches!(c, b'{' | b'[' | b'(') {
            depth += 1;
            current.push(c as char);
        } else if matches!(c, b'}' | b']' | b')') {
            depth -= 1;
            current.push(c as char);
        } else if c == b',' && depth == 0 {
            props.push(current.clone());
            current.clear();
        } else {
            current.push(c as char);
        }
    }
    if !current.trim().is_empty() {
        props.push(current);
    }
    props
}

/// Ensure all statements in a segment body end with semicolons to match SWC output.
/// SWC reprints from AST, so every statement gets a semicolon. Our optimizer preserves
/// source text, so we need to add them.
///
/// Uses AST parsing: wraps the body in `const __fn = <body>`, parses with oxc,
/// and inserts semicolons at the end of each statement that lacks one.
fn ensure_return_semicolons(body: &str) -> String {
    // Try AST-based approach first
    if let Some(result) = ensure_semicolons_ast(body) {
        return result;
    }
    // Fallback: line-based heuristic for return statements only
    ensure_return_semicolons_heuristic(body)
}

/// AST-based semicolon insertion: parse the segment body, find statement end
/// positions, and insert semicolons where missing.
fn ensure_semicolons_ast(body: &str) -> Option<String> {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // Wrap in a function assignment to make it parseable
    let wrapper_prefix = "const __qwik_fn = ";
    let wrapped = format!("{}{};", wrapper_prefix, body);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked || !parse_result.errors.is_empty() {
        return None;
    }

    // Find the arrow/function body
    let program = &parse_result.program;
    let first_stmt = program.body.first()?;
    let Statement::VariableDeclaration(decl) = first_stmt else { return None };
    let declarator = decl.declarations.first()?;
    let init = declarator.init.as_ref()?;

    // Get the function body - only handle block body arrows (not expression bodies)
    let fn_body_stmts = match init {
        Expression::ArrowFunctionExpression(arrow) => {
            // Only process block body arrows: `()=>{ ... }`
            // Skip expression body arrows: `()=>expr` (these have `expression: true`)
            if arrow.expression {
                return None; // expression body, no statements to add semicolons to
            }
            &arrow.body.statements
        }
        _ => return None,
    };

    if fn_body_stmts.is_empty() {
        return None;
    }

    // Collect positions where semicolons need to be inserted.
    // We work with offsets relative to the original `body` string.
    let prefix_len = wrapper_prefix.len();
    let mut insertions: Vec<usize> = Vec::new();

    for stmt in fn_body_stmts {
        let stmt_end = stmt.span().end as usize;
        // Convert from wrapped position to body position
        if stmt_end <= prefix_len || stmt_end > prefix_len + body.len() {
            continue;
        }
        let body_offset = stmt_end - prefix_len;

        // Check if the character at stmt_end-1 in the wrapped source is already a `;`
        let wrapped_bytes = wrapped.as_bytes();
        if stmt_end > 0 && stmt_end <= wrapped_bytes.len() {
            // AST span end is exclusive, so the last char of the statement is at end-1
            let last_char_pos = stmt_end - 1;
            if wrapped_bytes[last_char_pos] == b';' {
                continue; // Already has semicolon
            }
        }

        // Check if there's already a semicolon at this position in the body
        if body_offset > 0 && body_offset <= body.len() {
            let c = body.as_bytes()[body_offset - 1];
            if c == b';' {
                continue;
            }
        }

        // Don't add semicolons after block-ending statements (if, for, while, try, etc.)
        // These are identified by ending with `}`
        match stmt {
            Statement::IfStatement(_) |
            Statement::ForStatement(_) |
            Statement::ForInStatement(_) |
            Statement::ForOfStatement(_) |
            Statement::WhileStatement(_) |
            Statement::DoWhileStatement(_) |
            Statement::SwitchStatement(_) |
            Statement::TryStatement(_) |
            Statement::LabeledStatement(_) |
            Statement::BlockStatement(_) => continue,
            Statement::FunctionDeclaration(_) => continue,
            Statement::ClassDeclaration(_) => continue,
            _ => {}
        }

        insertions.push(body_offset);
    }

    if insertions.is_empty() {
        return None; // No changes needed
    }

    // Sort insertions in reverse order so we can insert without shifting offsets
    insertions.sort_unstable();
    insertions.dedup();

    let mut result = body.to_string();
    for &pos in insertions.iter().rev() {
        if pos <= result.len() {
            result.insert(pos, ';');
        }
    }

    Some(result)
}

/// Fallback line-based heuristic for return statement semicolons.
fn ensure_return_semicolons_heuristic(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("return ") && !trimmed.ends_with(';') {
            // Only add semicolon if the line ends with a "complete expression" character
            let last_char = trimmed.as_bytes()[trimmed.len() - 1];
            let looks_complete = matches!(last_char,
                b')' | b'"' | b'\'' | b'`' |   // closing paren, string
                b'0'..=b'9' |                    // number
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'  // identifier
            );
            if looks_complete {
                result.push_str(line);
                result.push(';');
            } else {
                result.push_str(line);
            }
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    if !body.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Public wrapper for minify_arrow, used by rewrite_parent.
pub fn minify_arrow_public(body: &str) -> String {
    minify_arrow(body)
}

/// Public wrapper for ensure_return_semicolons, used by rewrite_parent.
pub fn ensure_return_semicolons_public(body: &str) -> String {
    ensure_return_semicolons(body)
}

/// Minify arrow function syntax to match SWC output format.
/// Removes spaces around `=>` in ALL arrow functions in the body.
fn minify_arrow(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    // Track whether we're inside a string literal to avoid modifying strings
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_template = false;

    while i < len {
        // Track string state
        if !in_double_quote && !in_template && bytes[i] == b'\'' && (i == 0 || bytes[i - 1] != b'\\') {
            in_single_quote = !in_single_quote;
        } else if !in_single_quote && !in_template && bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_double_quote = !in_double_quote;
        } else if !in_single_quote && !in_double_quote && bytes[i] == b'`' && (i == 0 || bytes[i - 1] != b'\\') {
            in_template = !in_template;
        }

        if in_single_quote || in_double_quote || in_template {
            result.push(bytes[i] as char);
            i += 1;
            continue;
        }

        // Look for ` => ` or ` =>\n` or ` =>` patterns preceded by `)`
        if bytes[i] == b' ' && i + 3 < len && bytes[i + 1] == b'=' && bytes[i + 2] == b'>' {
            // Check that the char before the space is `)` (possibly after trimming)
            let before_ok = if i > 0 {
                // Walk back through whitespace to find `)`
                let mut k = i - 1;
                while k > 0 && bytes[k] == b' ' { k -= 1; }
                bytes[k] == b')'
            } else {
                false
            };

            if before_ok {
                // Skip the leading space, emit `=>`
                result.push('=');
                result.push('>');
                i += 3; // skip ` =>`
                // If followed by a space (` => expr`), skip that space too
                if i < len && bytes[i] == b' ' {
                    i += 1;
                }
                continue;
            }
        }

        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// Collapse newlines after `=>` in expression arrow functions.
/// `()=>\n/*#__PURE__*/ ...` → `()=>/*#__PURE__*/ ...`
/// `()=>\n  expr` → `()=>expr`
fn collapse_arrow_newline(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && bytes[i] == b'=' && bytes[i + 1] == b'>' {
            result.push('=');
            result.push('>');
            i += 2;
            // Skip whitespace (including newlines) after `=>`
            // but only if what follows is NOT `{` (block body)
            let mut j = i;
            while j < len && (bytes[j] == b' ' || bytes[j] == b'\n' || bytes[j] == b'\t' || bytes[j] == b'\r') {
                j += 1;
            }
            if j < len && bytes[j] != b'{' && j > i {
                // Expression body: collapse whitespace
                i = j;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Normalize destructuring whitespace to match SWC output.
/// Adds spaces inside braces: `{x}` → `{ x }`, `{x, y}` → `{ x, y }`
/// But only in parameter/destructuring contexts, not in block bodies.
pub fn normalize_destructuring_spaces(body: &str) -> String {
    let mut result = String::with_capacity(body.len() + 32);
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'{' {
            // Check if this is a destructuring pattern (not a block body or JSX expression)
            // Heuristic: if preceded by `(`, `,`, `=`, `[` or at start of params, it's a candidate
            let before = if i > 0 { bytes[i - 1] } else { b'(' };
            let is_candidate = matches!(before, b'(' | b',' | b'=' | b'[' | b' ');

            if is_candidate {
                // Find matching }
                let mut depth = 1;
                let mut j = i + 1;
                while j < len && depth > 0 {
                    if bytes[j] == b'{' { depth += 1; }
                    if bytes[j] == b'}' { depth -= 1; }
                    j += 1;
                }
                if depth == 0 {
                    let inner = &body[i + 1..j - 1];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() && !trimmed.contains('\n') {
                        // Distinguish destructuring `({x})` from JSX expression `onClick={x}`.
                        // After `=`, single-item `{ident}` is a JSX expression container and
                        // should NOT get spaces.  After `(` or `,` or `[`, it's always a
                        // destructuring pattern and should get spaces.  After `=`, only add
                        // spaces if the content has commas/colons/spread (real destructuring
                        // like `const { a, b } = ...`).
                        let is_after_equals = before == b'=';
                        // Skip JSX spread syntax: `{...expr}` should NOT get spaces
                        let is_jsx_spread = trimmed.starts_with("...")
                            && !trimmed.contains(',')
                            && !trimmed.contains(':');
                        let skip = is_jsx_spread
                            || (is_after_equals
                                && !trimmed.contains(',')
                                && !trimmed.contains(':')
                                && !trimmed.starts_with("..."));
                        if !skip {
                            result.push_str("{ ");
                            result.push_str(trimmed);
                            result.push_str(" }");
                            i = j;
                            continue;
                        }
                    }
                }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// Strip `/* @qwik-disable-next-line ... */` directive comments from segment bodies.
/// These are authoring-time directives that SWC removes during compilation.
fn strip_qwik_directive_comments(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("/* @qwik-disable") && trimmed.ends_with("*/") {
            // Skip this entire line (directive comment)
            continue;
        }
        // Also handle single-line // @qwik-disable comments
        if trimmed.starts_with("// @qwik-disable") {
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

/// Normalize spaces around compound assignment operators to match SWC output.
/// Ensures `x+=y` becomes `x += y`, `x-=y` becomes `x -= y`, etc.
/// Only operates outside of string literals.
fn normalize_compound_assignment_spaces(body: &str) -> String {
    let mut result = String::with_capacity(body.len() + 32);
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_sq = false;
    let mut in_dq = false;
    let mut in_tpl = false;

    while i < len {
        // Track string state
        if !in_dq && !in_tpl && bytes[i] == b'\'' && (i == 0 || bytes[i - 1] != b'\\') {
            in_sq = !in_sq;
        } else if !in_sq && !in_tpl && bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_dq = !in_dq;
        } else if !in_sq && !in_dq && bytes[i] == b'`' && (i == 0 || bytes[i - 1] != b'\\') {
            in_tpl = !in_tpl;
        }

        if in_sq || in_dq || in_tpl {
            result.push(bytes[i] as char);
            i += 1;
            continue;
        }

        // Check for compound assignment: +=, -=, *=, /=, %=, **=, &&=, ||=, ??=, <<=, >>=, >>>=
        // But NOT ==, !=, <=, >=, =>, ===, !==
        if i + 1 < len && bytes[i + 1] == b'=' {
            let op_char = bytes[i];
            let is_compound = matches!(op_char, b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^');
            if is_compound {
                // Make sure the character before is an identifier char (not another operator)
                let before_is_ident = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'$' || bytes[i - 1] == b'.' || bytes[i - 1] == b')' || bytes[i - 1] == b']');
                // Make sure char after = is not = (i.e., not ==, !== etc.)
                let after_not_eq = i + 2 >= len || bytes[i + 2] != b'=';
                if before_is_ident && after_not_eq {
                    // Ensure spaces: `x += y`
                    let needs_space_before = !result.ends_with(' ');
                    if needs_space_before {
                        result.push(' ');
                    }
                    result.push(op_char as char);
                    result.push('=');
                    i += 2;
                    // Ensure space after
                    if i < len && bytes[i] != b' ' {
                        result.push(' ');
                    }
                    continue;
                }
            }
        }

        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// Strip TypeScript type annotations from segment body text.
///
/// Uses oxc_parser to parse the body as a TS expression, then collects spans
/// of type annotations (parameter types, return types, type assertions, etc.)
/// and removes them from the source text.
fn strip_ts_annotations(body: &str) -> String {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // Wrap body in a variable declaration so it parses as a complete program
    let wrapper = format!("const __q = {};", body);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_result = Parser::new(&allocator, &wrapper, source_type).parse();

    // Collect spans to remove (relative to the wrapper string)
    let mut spans_to_remove: Vec<(usize, usize)> = Vec::new();

    // Walk the AST to find type annotations
    collect_ts_annotation_spans(&parser_result.program, &mut spans_to_remove);

    if spans_to_remove.is_empty() {
        return body.to_string();
    }

    // The body starts at this offset in the wrapper
    let body_offset = "const __q = ".len();

    // Convert spans from wrapper-relative to body-relative and filter
    let mut body_spans: Vec<(usize, usize)> = spans_to_remove
        .iter()
        .filter_map(|&(start, end)| {
            if start >= body_offset && end > start {
                Some((start - body_offset, end - body_offset))
            } else {
                None
            }
        })
        .filter(|&(start, end)| start < body.len() && end <= body.len())
        .collect();

    if body_spans.is_empty() {
        return body.to_string();
    }

    // Sort spans by start position descending so we can remove from end to start
    body_spans.sort_by(|a, b| b.0.cmp(&a.0));
    // Merge overlapping spans
    body_spans.dedup();

    let mut result = body.to_string();
    for (start, end) in body_spans {
        if start < result.len() && end <= result.len() {
            result = format!("{}{}", &result[..start], &result[end..]);
        }
    }

    result
}

/// Walk AST and collect spans of TypeScript type annotations to remove.
fn collect_ts_annotation_spans(
    program: &oxc_ast::ast::Program<'_>,
    spans: &mut Vec<(usize, usize)>,
) {
    use oxc_ast::ast::*;

    for stmt in &program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let Some(init) = &declarator.init {
                    collect_expr_ts_spans(init, spans);
                }
            }
        }
    }
}

/// Collect TS annotation spans from an expression (arrow function, function expr, etc.)
fn collect_expr_ts_spans(
    expr: &oxc_ast::ast::Expression<'_>,
    spans: &mut Vec<(usize, usize)>,
) {
    use oxc_ast::ast::*;

    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            // Return type annotation
            if let Some(ref ret) = arrow.return_type {
                spans.push((ret.span.start as usize, ret.span.end as usize));
            }
            // Type parameters
            if let Some(ref tp) = arrow.type_parameters {
                spans.push((tp.span.start as usize, tp.span.end as usize));
            }
            // Parameter type annotations
            collect_formal_params_ts_spans(&arrow.params, spans);
            // Recurse into body
            for stmt in &arrow.body.statements {
                collect_stmt_ts_spans(stmt, spans);
            }
        }
        Expression::FunctionExpression(func) => {
            if let Some(ref ret) = func.return_type {
                spans.push((ret.span.start as usize, ret.span.end as usize));
            }
            if let Some(ref tp) = func.type_parameters {
                spans.push((tp.span.start as usize, tp.span.end as usize));
            }
            collect_formal_params_ts_spans(&func.params, spans);
            if let Some(ref body) = func.body {
                for stmt in &body.statements {
                    collect_stmt_ts_spans(stmt, spans);
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_expr_ts_spans(&paren.expression, spans);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_expr_ts_spans(e, spans);
            }
        }
        Expression::TSAsExpression(ts_as) => {
            let expr_end = ts_as.expression.span().end as usize;
            let as_end = ts_as.span.end as usize;
            if as_end > expr_end {
                spans.push((expr_end, as_end));
            }
            collect_expr_ts_spans(&ts_as.expression, spans);
        }
        Expression::TSSatisfiesExpression(sat) => {
            let expr_end = sat.expression.span().end as usize;
            let sat_end = sat.span.end as usize;
            if sat_end > expr_end {
                spans.push((expr_end, sat_end));
            }
            collect_expr_ts_spans(&sat.expression, spans);
        }
        Expression::TSNonNullExpression(nn) => {
            let expr_end = nn.expression.span().end as usize;
            let nn_end = nn.span.end as usize;
            if nn_end > expr_end {
                spans.push((expr_end, nn_end));
            }
            collect_expr_ts_spans(&nn.expression, spans);
        }
        Expression::TSTypeAssertion(ta) => {
            let ta_start = ta.span.start as usize;
            let expr_start = ta.expression.span().start as usize;
            if expr_start > ta_start {
                spans.push((ta_start, expr_start));
            }
            collect_expr_ts_spans(&ta.expression, spans);
        }
        Expression::CallExpression(call) => {
            collect_expr_ts_spans(&call.callee, spans);
            // Strip type arguments on call: `foo<Type>(args)`
            if let Some(ref tp) = call.type_arguments {
                spans.push((tp.span.start as usize, tp.span.end as usize));
            }
            for arg in &call.arguments {
                match arg {
                    Argument::SpreadElement(s) => collect_expr_ts_spans(&s.argument, spans),
                    _ => collect_expr_ts_spans(arg.to_expression(), spans),
                }
            }
        }
        Expression::NewExpression(ne) => {
            collect_expr_ts_spans(&ne.callee, spans);
            if let Some(ref tp) = ne.type_arguments {
                spans.push((tp.span.start as usize, tp.span.end as usize));
            }
            for arg in &ne.arguments {
                match arg {
                    Argument::SpreadElement(s) => collect_expr_ts_spans(&s.argument, spans),
                    _ => collect_expr_ts_spans(arg.to_expression(), spans),
                }
            }
        }
        Expression::ConditionalExpression(cond) => {
            collect_expr_ts_spans(&cond.test, spans);
            collect_expr_ts_spans(&cond.consequent, spans);
            collect_expr_ts_spans(&cond.alternate, spans);
        }
        Expression::AssignmentExpression(assign) => {
            collect_expr_ts_spans(&assign.right, spans);
        }
        Expression::BinaryExpression(bin) => {
            collect_expr_ts_spans(&bin.left, spans);
            collect_expr_ts_spans(&bin.right, spans);
        }
        Expression::LogicalExpression(log) => {
            collect_expr_ts_spans(&log.left, spans);
            collect_expr_ts_spans(&log.right, spans);
        }
        Expression::UnaryExpression(un) => {
            collect_expr_ts_spans(&un.argument, spans);
        }
        Expression::UpdateExpression(_up) => {
            // UpdateExpression argument is SimpleAssignmentTarget, not Expression - skip
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(s) => collect_expr_ts_spans(&s.argument, spans),
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = elem.as_expression() {
                            collect_expr_ts_spans(e, spans);
                        }
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        collect_expr_ts_spans(&p.value, spans);
                    }
                    ObjectPropertyKind::SpreadProperty(s) => {
                        collect_expr_ts_spans(&s.argument, spans);
                    }
                }
            }
        }
        Expression::TemplateLiteral(tl) => {
            for expr in &tl.expressions {
                collect_expr_ts_spans(expr, spans);
            }
        }
        Expression::TaggedTemplateExpression(tte) => {
            collect_expr_ts_spans(&tte.tag, spans);
            for expr in &tte.quasi.expressions {
                collect_expr_ts_spans(expr, spans);
            }
        }
        Expression::YieldExpression(y) => {
            if let Some(ref arg) = y.argument {
                collect_expr_ts_spans(arg, spans);
            }
        }
        Expression::AwaitExpression(a) => {
            collect_expr_ts_spans(&a.argument, spans);
        }
        _ => {}
    }
}

/// Collect TS annotation spans from function formal parameters.
fn collect_formal_params_ts_spans(
    params: &oxc_ast::ast::FormalParameters<'_>,
    spans: &mut Vec<(usize, usize)>,
) {
    for param in &params.items {
        collect_binding_pattern_ts_spans(&param.pattern, spans);
    }
}

/// Collect TS annotation spans from a binding pattern (param declarations).
fn collect_binding_pattern_ts_spans(
    pat: &oxc_ast::ast::BindingPattern<'_>,
    spans: &mut Vec<(usize, usize)>,
) {
    if let Some(ref ann) = pat.type_annotation {
        spans.push((ann.span.start as usize, ann.span.end as usize));
    }
}

/// Collect TS annotation spans from statements (for nested functions, etc.)
fn collect_stmt_ts_spans(
    stmt: &oxc_ast::ast::Statement<'_>,
    spans: &mut Vec<(usize, usize)>,
) {
    use oxc_ast::ast::*;

    match stmt {
        Statement::VariableDeclaration(decl) => {
            for d in &decl.declarations {
                if let Some(ref ann) = d.id.type_annotation {
                    spans.push((ann.span.start as usize, ann.span.end as usize));
                }
                if let Some(ref init) = d.init {
                    collect_expr_ts_spans(init, spans);
                }
            }
        }
        Statement::ExpressionStatement(es) => {
            collect_expr_ts_spans(&es.expression, spans);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                collect_expr_ts_spans(arg, spans);
            }
        }
        Statement::FunctionDeclaration(func) => {
            if let Some(ref ret) = func.return_type {
                spans.push((ret.span.start as usize, ret.span.end as usize));
            }
            if let Some(ref tp) = func.type_parameters {
                spans.push((tp.span.start as usize, tp.span.end as usize));
            }
            collect_formal_params_ts_spans(&func.params, spans);
            if let Some(ref body) = func.body {
                for s in &body.statements {
                    collect_stmt_ts_spans(s, spans);
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_expr_ts_spans(&if_stmt.test, spans);
            collect_stmt_ts_spans(&if_stmt.consequent, spans);
            if let Some(ref alt) = if_stmt.alternate {
                collect_stmt_ts_spans(alt, spans);
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_stmt_ts_spans(s, spans);
            }
        }
        Statement::ForStatement(f) => {
            if let Some(ref test) = f.test {
                collect_expr_ts_spans(test, spans);
            }
            if let Some(ref update) = f.update {
                collect_expr_ts_spans(update, spans);
            }
            collect_stmt_ts_spans(&f.body, spans);
        }
        Statement::ForInStatement(f) => {
            collect_expr_ts_spans(&f.right, spans);
            collect_stmt_ts_spans(&f.body, spans);
        }
        Statement::ForOfStatement(f) => {
            collect_expr_ts_spans(&f.right, spans);
            collect_stmt_ts_spans(&f.body, spans);
        }
        Statement::WhileStatement(w) => {
            collect_expr_ts_spans(&w.test, spans);
            collect_stmt_ts_spans(&w.body, spans);
        }
        Statement::DoWhileStatement(dw) => {
            collect_stmt_ts_spans(&dw.body, spans);
            collect_expr_ts_spans(&dw.test, spans);
        }
        Statement::TryStatement(t) => {
            for s in &t.block.body {
                collect_stmt_ts_spans(s, spans);
            }
            if let Some(ref handler) = t.handler {
                if let Some(ref param) = handler.param {
                    collect_binding_pattern_ts_spans(&param.pattern, spans);
                }
                for s in &handler.body.body {
                    collect_stmt_ts_spans(s, spans);
                }
            }
            if let Some(ref fin) = t.finalizer {
                for s in &fin.body {
                    collect_stmt_ts_spans(s, spans);
                }
            }
        }
        Statement::ThrowStatement(t) => {
            collect_expr_ts_spans(&t.argument, spans);
        }
        Statement::SwitchStatement(sw) => {
            collect_expr_ts_spans(&sw.discriminant, spans);
            for case in &sw.cases {
                if let Some(ref test) = case.test {
                    collect_expr_ts_spans(test, spans);
                }
                for s in &case.consequent {
                    collect_stmt_ts_spans(s, spans);
                }
            }
        }
        Statement::TSTypeAliasDeclaration(ta) => {
            spans.push((ta.span.start as usize, ta.span.end as usize));
        }
        Statement::TSInterfaceDeclaration(iface) => {
            spans.push((iface.span.start as usize, iface.span.end as usize));
        }
        _ => {}
    }
}

/// Dead code elimination for segment bodies.
///
/// Parses the body, identifies unused declarations, and removes them.
/// This matches SWC's behavior of stripping:
/// - Variable declarations whose names aren't referenced elsewhere
/// - Function/class declarations whose names aren't referenced
/// - Empty try/catch blocks
/// - `if (false) { ... }` branches
fn eliminate_dead_code_in_body(body: &str) -> String {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // Wrap the body in a variable declaration so it parses as a complete program
    let wrapped = format!("const __qwik_dce_fn = {};", body);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked {
        return body.to_string();
    }

    // Find the function body statements
    let fn_body_stmts = dce_extract_fn_body_stmts(&parse_result.program);
    if fn_body_stmts.is_empty() {
        return body.to_string();
    }

    // Collect all referenced identifiers in the body (excluding declaration sites)
    let mut all_refs = std::collections::HashSet::new();
    for stmt in &fn_body_stmts {
        dce_collect_references_in_stmt(stmt, &mut all_refs);
    }

    // Also collect references from the function parameters (they are always "used")
    let fn_params = dce_extract_fn_params(&parse_result.program);
    for p in &fn_params {
        all_refs.insert(p.clone());
    }

    // Now identify which statements to remove
    // We need to compute spans relative to the original body text
    let prefix_len = "const __qwik_dce_fn = ".len();

    // Collect ranges to remove or replace: (start, end, Option<replacement>)
    // None = remove entirely, Some(text) = replace with text
    let mut edits: Vec<(usize, usize, Option<String>)> = Vec::new();

    for stmt in &fn_body_stmts {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                // Check if ALL declarators in this declaration are unused
                let all_unused = decl.declarations.iter().all(|declarator| {
                    let names = dce_collect_pattern_names(&declarator.id);
                    names.iter().all(|n| !all_refs.contains(n.as_str()))
                });
                if all_unused {
                    // Check if any initializer might have side effects.
                    // Also, destructuring patterns (object/array) have implicit side effects
                    // because they invoke property access or iterators.
                    let has_side_effects = decl.declarations.iter().any(|d| {
                        // Destructuring patterns always have side effects when there's an initializer
                        let is_destructuring = matches!(
                            d.id.kind,
                            oxc_ast::ast::BindingPatternKind::ObjectPattern(_)
                            | oxc_ast::ast::BindingPatternKind::ArrayPattern(_)
                        );
                        if is_destructuring && d.init.is_some() {
                            return true;
                        }
                        d.init.as_ref().is_some_and(|init| dce_expr_has_side_effects(init))
                    });
                    let start = decl.span.start as usize;
                    let end = decl.span.end as usize;
                    if start >= prefix_len {
                        if !has_side_effects {
                            // Safe to remove entirely
                            edits.push((start - prefix_len, end - prefix_len, None));
                        } else {
                            // Has side effects: convert to expression statements.
                            // `const x = expr;` => `expr;`
                            // Only for single-identifier patterns with side-effectful init.
                            // Skip destructuring (too complex to safely convert).
                            let mut replacement_parts: Vec<String> = Vec::new();
                            let mut can_convert = true;
                            for d in &decl.declarations {
                                let is_destructuring = matches!(
                                    d.id.kind,
                                    oxc_ast::ast::BindingPatternKind::ObjectPattern(_)
                                    | oxc_ast::ast::BindingPatternKind::ArrayPattern(_)
                                );
                                if is_destructuring {
                                    can_convert = false;
                                    break;
                                }
                                if let Some(ref init) = d.init {
                                    let init_start = init.span().start as usize;
                                    let init_end = init.span().end as usize;
                                    if init_start >= prefix_len && init_end >= prefix_len
                                        && init_end - prefix_len <= body.len()
                                    {
                                        let expr_text = &body[init_start - prefix_len..init_end - prefix_len];
                                        replacement_parts.push(expr_text.to_string());
                                    } else {
                                        can_convert = false;
                                        break;
                                    }
                                }
                                // No init = `const x;` which is side-effect-free, skip
                            }
                            if can_convert && !replacement_parts.is_empty() {
                                let replacement = format!("{};", replacement_parts.join(";\n"));
                                edits.push((start - prefix_len, end - prefix_len, Some(replacement)));
                            }
                        }
                    }
                }
            }
            Statement::FunctionDeclaration(fn_decl) => {
                if let Some(ref id) = fn_decl.id {
                    if !all_refs.contains(id.name.as_str()) {
                        let start = fn_decl.span.start as usize;
                        let end = fn_decl.span.end as usize;
                        if start >= prefix_len {
                            edits.push((start - prefix_len, end - prefix_len, None));
                        }
                    }
                }
            }
            Statement::ClassDeclaration(cls) => {
                if let Some(ref id) = cls.id {
                    if !all_refs.contains(id.name.as_str()) {
                        // Only remove truly simple classes (no side effects)
                        let has_effects = dce_class_has_side_effects(cls);
                        if !has_effects {
                            let start = cls.span.start as usize;
                            let end = cls.span.end as usize;
                            if start >= prefix_len {
                                edits.push((start - prefix_len, end - prefix_len, None));
                            }
                        }
                    }
                }
            }
            Statement::TryStatement(try_stmt) => {
                // Remove empty try/catch blocks: `try {} catch(e) {}`
                let try_empty = try_stmt.block.body.is_empty();
                let catch_empty = try_stmt.handler.as_ref().is_some_and(|h| h.body.body.is_empty());
                let no_finally = try_stmt.finalizer.is_none();
                if try_empty && catch_empty && no_finally {
                    let start = try_stmt.span.start as usize;
                    let end = try_stmt.span.end as usize;
                    if start >= prefix_len {
                        edits.push((start - prefix_len, end - prefix_len, None));
                    }
                }
            }
            Statement::IfStatement(if_stmt) => {
                // Remove `if (false) { ... }` entirely (when no else branch)
                if dce_is_literal_false(&if_stmt.test) && if_stmt.alternate.is_none() {
                    let start = if_stmt.span.start as usize;
                    let end = if_stmt.span.end as usize;
                    if start >= prefix_len {
                        edits.push((start - prefix_len, end - prefix_len, None));
                    }
                }
            }
            _ => {}
        }
    }

    if edits.is_empty() {
        // Still need to check for nested if(false) blocks even if no top-level edits
        let mut result = remove_if_false_blocks(&body.to_string());
        result = collapse_empty_blocks(&result);
        return result;
    }

    // Sort edits by start position descending so we can apply from end to start
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    edits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let mut result = body.to_string();
    for (start, end, replacement) in &edits {
        if *end <= result.len() && *start <= *end {
            let mut remove_end = *end;
            let bytes = result.as_bytes();
            // Skip trailing semicolon if present
            if remove_end < bytes.len() && bytes[remove_end] == b';' {
                remove_end += 1;
            }
            // Skip trailing whitespace and newlines
            while remove_end < bytes.len() && (bytes[remove_end] == b'\n' || bytes[remove_end] == b'\r' || bytes[remove_end] == b'\t' || bytes[remove_end] == b' ') {
                remove_end += 1;
            }
            if let Some(repl) = replacement {
                // Replace: insert replacement text followed by newline
                result = format!("{}{}\n{}", &result[..*start], repl, &result[remove_end..]);
            } else {
                // Remove entirely
                result = format!("{}{}", &result[..*start], &result[remove_end..]);
            }
        }
    }

    // After AST-based DCE, do a text-based pass to remove `if (false) { ... }` blocks
    // at any nesting level. The AST pass only handles top-level statements, but these
    // blocks can appear inside nested arrows (e.g., `useMount$(() => { if (false) { ... } })`).
    result = remove_if_false_blocks(&result);

    // Collapse empty blocks that have only whitespace: `{\n}` or `{\n\t\n}` → `{}`
    result = collapse_empty_blocks(&result);

    result
}

/// Remove `if (false) { ... }` blocks from source text at any nesting level.
/// Handles the pattern: optional whitespace, `if`, whitespace, `(false)`, whitespace, `{ ... }`.
/// Does NOT remove if there's an `else` branch.
fn remove_if_false_blocks(source: &str) -> String {
    let mut result = source.to_string();
    loop {
        // Find `if (false)` pattern
        let Some(if_pos) = result.find("if (false)") else {
            // Also try `if(false)` without space
            if let Some(if_pos) = result.find("if(false)") {
                let after = &result[if_pos + 9..].trim_start();
                if after.starts_with('{') {
                    let block_start = result[if_pos + 9..].find('{').unwrap() + if_pos + 9;
                    if let Some(block_end) = find_matching_brace(&result, block_start) {
                        // Check no `else` after the block
                        let after_block = result[block_end + 1..].trim_start();
                        if !after_block.starts_with("else") {
                            // Remove everything from if to end of block, plus trailing whitespace
                            let mut remove_end = block_end + 1;
                            let bytes = result.as_bytes();
                            while remove_end < bytes.len() && matches!(bytes[remove_end], b'\n' | b'\r' | b'\t' | b' ') {
                                remove_end += 1;
                            }
                            // Also remove leading whitespace on the same line
                            let mut remove_start = if_pos;
                            while remove_start > 0 && matches!(result.as_bytes()[remove_start - 1], b'\t' | b' ') {
                                remove_start -= 1;
                            }
                            result = format!("{}{}", &result[..remove_start], &result[remove_end..]);
                            continue;
                        }
                    }
                }
                break;
            }
            break;
        };

        let pattern_end = if_pos + 10; // past "if (false)"
        let after = result[pattern_end..].trim_start();
        if !after.starts_with('{') {
            // Not a block statement, skip this occurrence
            // Replace the first occurrence to continue searching
            break;
        }

        let block_start = result[pattern_end..].find('{').unwrap() + pattern_end;
        let Some(block_end) = find_matching_brace(&result, block_start) else {
            break;
        };

        // Check no `else` after the block
        let after_block = result[block_end + 1..].trim_start();
        if after_block.starts_with("else") {
            break;
        }

        // Remove everything from if to end of block, plus trailing whitespace/newline
        let mut remove_end = block_end + 1;
        let bytes = result.as_bytes();
        while remove_end < bytes.len() && matches!(bytes[remove_end], b'\n' | b'\r' | b'\t' | b' ') {
            remove_end += 1;
        }
        // Also remove leading whitespace on the same line
        let mut remove_start = if_pos;
        while remove_start > 0 && matches!(result.as_bytes()[remove_start - 1], b'\t' | b' ') {
            remove_start -= 1;
        }
        result = format!("{}{}", &result[..remove_start], &result[remove_end..]);
    }
    result
}

/// Collapse empty blocks that contain only whitespace: `{\n}`, `{\n\t\n}` → `{}`.
fn collapse_empty_blocks(source: &str) -> String {
    let mut result = source.to_string();
    loop {
        let bytes = result.as_bytes();
        let mut changed = false;
        for i in 0..bytes.len() {
            if bytes[i] == b'{' {
                // Check if everything between { and matching } is whitespace
                let mut j = i + 1;
                while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'}' && j > i + 1 {
                    // Found `{ ... }` with only whitespace inside — collapse to `{}`
                    result = format!("{}{{}}{}", &result[..i], &result[j + 1..]);
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
    }
    result
}

/// Find the position of the closing `}` that matches the `{` at `open_pos`.
fn find_matching_brace(source: &str, open_pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes[open_pos] != b'{' {
        return None;
    }
    let mut depth = 1;
    let mut i = open_pos + 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\'' | b'"' | b'`' => {
                // Skip string literals
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' { i += 1; } // skip escaped char
                    i += 1;
                }
            }
            _ => {}
        }
        if depth == 0 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Extract the function body statements from a wrapped program.
fn dce_extract_fn_body_stmts<'a>(program: &'a oxc_ast::ast::Program<'a>) -> Vec<&'a oxc_ast::ast::Statement<'a>> {
    use oxc_ast::ast::*;
    for stmt in &program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let Some(ref init) = declarator.init {
                    return dce_extract_body_stmts_from_expr(init);
                }
            }
        }
    }
    Vec::new()
}

fn dce_extract_body_stmts_from_expr<'a>(expr: &'a oxc_ast::ast::Expression<'a>) -> Vec<&'a oxc_ast::ast::Statement<'a>> {
    use oxc_ast::ast::*;
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            arrow.body.statements.iter().collect()
        }
        Expression::FunctionExpression(fn_expr) => {
            if let Some(ref body) = fn_expr.body {
                body.statements.iter().collect()
            } else {
                Vec::new()
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            dce_extract_body_stmts_from_expr(&paren.expression)
        }
        _ => Vec::new(),
    }
}

/// Extract parameter names from the function.
fn dce_extract_fn_params(program: &oxc_ast::ast::Program) -> Vec<String> {
    use oxc_ast::ast::*;
    let mut params = Vec::new();
    for stmt in &program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let Some(ref init) = declarator.init {
                    dce_extract_params_from_expr(init, &mut params);
                }
            }
        }
    }
    params
}

fn dce_extract_params_from_expr(expr: &oxc_ast::ast::Expression, params: &mut Vec<String>) {
    use oxc_ast::ast::*;
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            for param in &arrow.params.items {
                let mut names = std::collections::HashSet::new();
                collect_binding_pattern_names(&param.pattern, &mut names);
                params.extend(names);
            }
        }
        Expression::FunctionExpression(fn_expr) => {
            for param in &fn_expr.params.items {
                let mut names = std::collections::HashSet::new();
                collect_binding_pattern_names(&param.pattern, &mut names);
                params.extend(names);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            dce_extract_params_from_expr(&paren.expression, params);
        }
        _ => {}
    }
}

/// Collect all identifier references in a statement (NOT declaration sites).
fn dce_collect_references_in_stmt(stmt: &oxc_ast::ast::Statement, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match stmt {
        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                // Collect refs from initializer, but NOT the binding name
                if let Some(ref init) = declarator.init {
                    dce_collect_references_in_expr(init, refs);
                }
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            dce_collect_references_in_expr(&expr_stmt.expression, refs);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                dce_collect_references_in_expr(arg, refs);
            }
        }
        Statement::IfStatement(if_stmt) => {
            dce_collect_references_in_expr(&if_stmt.test, refs);
            dce_collect_references_in_stmt(&if_stmt.consequent, refs);
            if let Some(ref alt) = if_stmt.alternate {
                dce_collect_references_in_stmt(alt, refs);
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                dce_collect_references_in_stmt(s, refs);
            }
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(ref init) = for_stmt.init {
                match init {
                    ForStatementInit::VariableDeclaration(decl) => {
                        for d in &decl.declarations {
                            if let Some(ref init_expr) = d.init {
                                dce_collect_references_in_expr(init_expr, refs);
                            }
                            let names = dce_collect_pattern_names(&d.id);
                            refs.extend(names);
                        }
                    }
                    _ => {
                        if let Some(expr) = init.as_expression() {
                            dce_collect_references_in_expr(expr, refs);
                        }
                    }
                }
            }
            if let Some(ref test) = for_stmt.test {
                dce_collect_references_in_expr(test, refs);
            }
            if let Some(ref update) = for_stmt.update {
                dce_collect_references_in_expr(update, refs);
            }
            dce_collect_references_in_stmt(&for_stmt.body, refs);
        }
        Statement::ForInStatement(for_in) => {
            dce_collect_references_in_expr(&for_in.right, refs);
            if let ForStatementLeft::VariableDeclaration(ref decl) = for_in.left {
                for d in &decl.declarations {
                    let names = dce_collect_pattern_names(&d.id);
                    refs.extend(names);
                }
            }
            dce_collect_references_in_stmt(&for_in.body, refs);
        }
        Statement::ForOfStatement(for_of) => {
            dce_collect_references_in_expr(&for_of.right, refs);
            if let ForStatementLeft::VariableDeclaration(ref decl) = for_of.left {
                for d in &decl.declarations {
                    let names = dce_collect_pattern_names(&d.id);
                    refs.extend(names);
                }
            }
            dce_collect_references_in_stmt(&for_of.body, refs);
        }
        Statement::WhileStatement(w) => {
            dce_collect_references_in_expr(&w.test, refs);
            dce_collect_references_in_stmt(&w.body, refs);
        }
        Statement::DoWhileStatement(dw) => {
            dce_collect_references_in_expr(&dw.test, refs);
            dce_collect_references_in_stmt(&dw.body, refs);
        }
        Statement::TryStatement(try_stmt) => {
            for s in &try_stmt.block.body {
                dce_collect_references_in_stmt(s, refs);
            }
            if let Some(ref handler) = try_stmt.handler {
                for s in &handler.body.body {
                    dce_collect_references_in_stmt(s, refs);
                }
            }
            if let Some(ref finalizer) = try_stmt.finalizer {
                for s in &finalizer.body {
                    dce_collect_references_in_stmt(s, refs);
                }
            }
        }
        Statement::ThrowStatement(t) => {
            dce_collect_references_in_expr(&t.argument, refs);
        }
        Statement::SwitchStatement(sw) => {
            dce_collect_references_in_expr(&sw.discriminant, refs);
            for case in &sw.cases {
                if let Some(ref test) = case.test {
                    dce_collect_references_in_expr(test, refs);
                }
                for s in &case.consequent {
                    dce_collect_references_in_stmt(s, refs);
                }
            }
        }
        Statement::FunctionDeclaration(fn_decl) => {
            // Don't add function name to refs (it's a declaration)
            if let Some(ref body) = fn_decl.body {
                for s in &body.statements {
                    dce_collect_references_in_stmt(s, refs);
                }
            }
        }
        Statement::ClassDeclaration(cls) => {
            dce_collect_references_in_class_body(&cls.body, refs);
        }
        Statement::LabeledStatement(labeled) => {
            dce_collect_references_in_stmt(&labeled.body, refs);
        }
        _ => {}
    }
}

/// Collect identifier references in an expression.
fn dce_collect_references_in_expr(expr: &oxc_ast::ast::Expression, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match expr {
        Expression::Identifier(id) => {
            refs.insert(id.name.to_string());
        }
        Expression::BinaryExpression(bin) => {
            dce_collect_references_in_expr(&bin.left, refs);
            dce_collect_references_in_expr(&bin.right, refs);
        }
        Expression::LogicalExpression(log) => {
            dce_collect_references_in_expr(&log.left, refs);
            dce_collect_references_in_expr(&log.right, refs);
        }
        Expression::UnaryExpression(unary) => {
            dce_collect_references_in_expr(&unary.argument, refs);
        }
        Expression::UpdateExpression(upd) => {
            match &upd.argument {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                    refs.insert(id.name.to_string());
                }
                SimpleAssignmentTarget::StaticMemberExpression(mem) => {
                    dce_collect_references_in_expr(&mem.object, refs);
                }
                SimpleAssignmentTarget::ComputedMemberExpression(mem) => {
                    dce_collect_references_in_expr(&mem.object, refs);
                    dce_collect_references_in_expr(&mem.expression, refs);
                }
                _ => {}
            }
        }
        Expression::AssignmentExpression(assign) => {
            dce_collect_references_in_assignment_target(&assign.left, refs);
            dce_collect_references_in_expr(&assign.right, refs);
        }
        Expression::ConditionalExpression(cond) => {
            dce_collect_references_in_expr(&cond.test, refs);
            dce_collect_references_in_expr(&cond.consequent, refs);
            dce_collect_references_in_expr(&cond.alternate, refs);
        }
        Expression::CallExpression(call) => {
            dce_collect_references_in_expr(&call.callee, refs);
            for arg in &call.arguments {
                match arg {
                    Argument::SpreadElement(spread) => {
                        dce_collect_references_in_expr(&spread.argument, refs);
                    }
                    _ => {
                        if let Some(expr) = arg.as_expression() {
                            dce_collect_references_in_expr(expr, refs);
                        }
                    }
                }
            }
        }
        Expression::NewExpression(new) => {
            dce_collect_references_in_expr(&new.callee, refs);
            for arg in &new.arguments {
                match arg {
                    Argument::SpreadElement(spread) => {
                        dce_collect_references_in_expr(&spread.argument, refs);
                    }
                    _ => {
                        if let Some(expr) = arg.as_expression() {
                            dce_collect_references_in_expr(expr, refs);
                        }
                    }
                }
            }
        }
        Expression::StaticMemberExpression(mem) => {
            dce_collect_references_in_expr(&mem.object, refs);
        }
        Expression::ComputedMemberExpression(mem) => {
            dce_collect_references_in_expr(&mem.object, refs);
            dce_collect_references_in_expr(&mem.expression, refs);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        dce_collect_references_in_expr(&spread.argument, refs);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(expr) = elem.as_expression() {
                            dce_collect_references_in_expr(expr, refs);
                        }
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if p.computed {
                            if let Some(expr) = p.key.as_expression() {
                                dce_collect_references_in_expr(expr, refs);
                            }
                        }
                        dce_collect_references_in_expr(&p.value, refs);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        dce_collect_references_in_expr(&spread.argument, refs);
                    }
                }
            }
        }
        Expression::ArrowFunctionExpression(arrow) => {
            // Don't collect param names as refs
            for stmt in &arrow.body.statements {
                dce_collect_references_in_stmt(stmt, refs);
            }
        }
        Expression::FunctionExpression(fn_expr) => {
            if let Some(ref body) = fn_expr.body {
                for stmt in &body.statements {
                    dce_collect_references_in_stmt(stmt, refs);
                }
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for expr in &tpl.expressions {
                dce_collect_references_in_expr(expr, refs);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            dce_collect_references_in_expr(&tagged.tag, refs);
            for expr in &tagged.quasi.expressions {
                dce_collect_references_in_expr(expr, refs);
            }
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                dce_collect_references_in_expr(expr, refs);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            dce_collect_references_in_expr(&paren.expression, refs);
        }
        Expression::AwaitExpression(aw) => {
            dce_collect_references_in_expr(&aw.argument, refs);
        }
        Expression::YieldExpression(y) => {
            if let Some(ref arg) = y.argument {
                dce_collect_references_in_expr(arg, refs);
            }
        }
        Expression::ClassExpression(cls) => {
            dce_collect_references_in_class_body(&cls.body, refs);
        }
        Expression::ChainExpression(chain) => {
            match &chain.expression {
                ChainElement::CallExpression(call) => {
                    dce_collect_references_in_expr(&call.callee, refs);
                    for arg in &call.arguments {
                        if let Some(expr) = arg.as_expression() {
                            dce_collect_references_in_expr(expr, refs);
                        }
                    }
                }
                ChainElement::StaticMemberExpression(mem) => {
                    dce_collect_references_in_expr(&mem.object, refs);
                }
                ChainElement::ComputedMemberExpression(mem) => {
                    dce_collect_references_in_expr(&mem.object, refs);
                    dce_collect_references_in_expr(&mem.expression, refs);
                }
                ChainElement::PrivateFieldExpression(pf) => {
                    dce_collect_references_in_expr(&pf.object, refs);
                }
                _ => {}
            }
        }
        Expression::JSXElement(jsx) => {
            dce_collect_references_in_jsx_element(jsx, refs);
        }
        Expression::JSXFragment(frag) => {
            for child in &frag.children {
                dce_collect_references_in_jsx_child(child, refs);
            }
        }
        _ => {}
    }
}

fn dce_collect_references_in_jsx_element(jsx: &oxc_ast::ast::JSXElement, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    // Collect refs from the tag name (component references)
    match &jsx.opening_element.name {
        JSXElementName::Identifier(id) => {
            if id.name.chars().next().is_some_and(|c| c.is_uppercase()) {
                refs.insert(id.name.to_string());
            }
        }
        JSXElementName::IdentifierReference(id) => {
            if id.name.chars().next().is_some_and(|c| c.is_uppercase()) {
                refs.insert(id.name.to_string());
            }
        }
        JSXElementName::MemberExpression(mem) => {
            dce_collect_refs_from_jsx_member(mem, refs);
        }
        _ => {}
    }
    // Collect refs from attributes
    for attr in &jsx.opening_element.attributes {
        match attr {
            JSXAttributeItem::Attribute(a) => {
                if let Some(ref value) = a.value {
                    if let JSXAttributeValue::ExpressionContainer(container) = value {
                        if let Some(expr) = container.expression.as_expression() {
                            dce_collect_references_in_expr(expr, refs);
                        }
                    }
                }
            }
            JSXAttributeItem::SpreadAttribute(spread) => {
                dce_collect_references_in_expr(&spread.argument, refs);
            }
        }
    }
    // Collect refs from children
    for child in &jsx.children {
        dce_collect_references_in_jsx_child(child, refs);
    }
}

fn dce_collect_references_in_jsx_child(child: &oxc_ast::ast::JSXChild, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match child {
        JSXChild::ExpressionContainer(container) => {
            if let Some(expr) = container.expression.as_expression() {
                dce_collect_references_in_expr(expr, refs);
            }
        }
        JSXChild::Element(elem) => {
            dce_collect_references_in_jsx_element(elem, refs);
        }
        JSXChild::Fragment(frag) => {
            for c in &frag.children {
                dce_collect_references_in_jsx_child(c, refs);
            }
        }
        JSXChild::Spread(spread) => {
            dce_collect_references_in_expr(&spread.expression, refs);
        }
        _ => {}
    }
}

fn dce_collect_refs_from_jsx_member(mem: &oxc_ast::ast::JSXMemberExpression, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match &mem.object {
        JSXMemberExpressionObject::IdentifierReference(id) => {
            refs.insert(id.name.to_string());
        }
        JSXMemberExpressionObject::MemberExpression(nested) => {
            dce_collect_refs_from_jsx_member(nested, refs);
        }
        _ => {}
    }
}

fn dce_collect_references_in_assignment_target(target: &oxc_ast::ast::AssignmentTarget, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            refs.insert(id.name.to_string());
        }
        AssignmentTarget::StaticMemberExpression(mem) => {
            dce_collect_references_in_expr(&mem.object, refs);
        }
        AssignmentTarget::ComputedMemberExpression(mem) => {
            dce_collect_references_in_expr(&mem.object, refs);
            dce_collect_references_in_expr(&mem.expression, refs);
        }
        _ => {}
    }
}

fn dce_collect_references_in_class_body(body: &oxc_ast::ast::ClassBody, refs: &mut std::collections::HashSet<String>) {
    use oxc_ast::ast::*;
    for elem in &body.body {
        match elem {
            ClassElement::MethodDefinition(method) => {
                if let Some(ref body) = method.value.body {
                    for s in &body.statements {
                        dce_collect_references_in_stmt(s, refs);
                    }
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(ref value) = prop.value {
                    dce_collect_references_in_expr(value, refs);
                }
            }
            _ => {}
        }
    }
}

/// Collect all names from a binding pattern.
fn dce_collect_pattern_names(pattern: &oxc_ast::ast::BindingPattern) -> Vec<String> {
    let mut names = std::collections::HashSet::new();
    collect_binding_pattern_names(pattern, &mut names);
    names.into_iter().collect()
}

/// Check if an expression has observable side effects.
fn dce_expr_has_side_effects(expr: &oxc_ast::ast::Expression) -> bool {
    use oxc_ast::ast::*;
    match expr {
        // Literals have no side effects
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => false,

        // Identifiers: no side effects (reads only)
        Expression::Identifier(_) => false,

        // Binary expressions: operands might have effects
        Expression::BinaryExpression(bin) => {
            dce_expr_has_side_effects(&bin.left) || dce_expr_has_side_effects(&bin.right)
        }

        // Unary (except delete/void): check argument
        Expression::UnaryExpression(unary) => {
            matches!(unary.operator, UnaryOperator::Delete | UnaryOperator::Void)
                || dce_expr_has_side_effects(&unary.argument)
        }

        // Array/Object literals: check all elements
        Expression::ArrayExpression(arr) => {
            arr.elements.iter().any(|e| {
                if let Some(expr) = e.as_expression() {
                    dce_expr_has_side_effects(expr)
                } else {
                    true
                }
            })
        }
        Expression::ObjectExpression(obj) => {
            obj.properties.iter().any(|p| match p {
                ObjectPropertyKind::ObjectProperty(prop) => {
                    prop.computed || dce_expr_has_side_effects(&prop.value)
                }
                ObjectPropertyKind::SpreadProperty(_) => true,
            })
        }

        // Template literals with no expressions: no side effects
        Expression::TemplateLiteral(tpl) => !tpl.expressions.is_empty(),

        // Everything else: assume side effects (calls, member access, etc.)
        _ => true,
    }
}

/// Check if a class declaration has side effects (property initializers, extends, etc.)
fn dce_class_has_side_effects(cls: &oxc_ast::ast::Class) -> bool {
    use oxc_ast::ast::*;
    if cls.super_class.is_some() {
        return true;
    }
    for elem in &cls.body.body {
        match elem {
            ClassElement::PropertyDefinition(prop) => {
                if prop.value.is_some() {
                    return true;
                }
            }
            ClassElement::StaticBlock(_) => return true,
            _ => {}
        }
    }
    false
}

/// Check if an expression is the literal `false`.
fn dce_is_literal_false(expr: &oxc_ast::ast::Expression) -> bool {
    use oxc_ast::ast::*;
    match expr {
        Expression::BooleanLiteral(b) => !b.value,
        Expression::ParenthesizedExpression(p) => dce_is_literal_false(&p.expression),
        _ => false,
    }
}

#[cfg(test)]
mod dce_tests {
    use super::*;

    #[test]
    fn test_dce_keeps_side_effectful_destructuring() {
        let body = "()=>{\n\tconst thing = useStore();\n\tconst {foo, bar} = foo();\n\treturn (<div>{thing}</div>);\n}";
        let result = eliminate_dead_code_in_body(body);
        assert!(result.contains("const {foo, bar} = foo()"), "DCE should keep destructuring with side effects. Got: {}", result);
        assert!(result.contains("useStore()"), "DCE should keep useStore(). Got: {}", result);
    }

    #[test]
    fn test_dce_removes_unused_literal_decl() {
        let body = "(decl1, {decl2}, [decl3])=>{\n\tconst {decl4, key: decl5} = this;\n\tlet [decl6, ...decl7] = stuff;\n\tconst decl8 = 1, decl9;\n\tfunction decl10(decl11, {decl12}, [decl13]) {}\n\tclass decl14 {\n\t\tmethod(decl15, {decl16}, [decl17]) {}\n\t}\n\ttry{}catch(decl18){}\n\ttry{}catch({decl19}){}\n}";
        let result = eliminate_dead_code_in_body(body);
        assert!(!result.contains("decl8"), "DCE should remove unused literal decl. Got: {}", result);
        assert!(!result.contains("decl10"), "DCE should remove unused function. Got: {}", result);
        assert!(!result.contains("decl14"), "DCE should remove unused class. Got: {}", result);
        assert!(!result.contains("decl18"), "DCE should remove empty try/catch. Got: {}", result);
        assert!(result.contains("decl4"), "DCE should keep destructuring from this. Got: {}", result);
        assert!(result.contains("decl6"), "DCE should keep destructuring from stuff. Got: {}", result);
    }

    #[test]
    fn test_dce_functional_component() {
        let body = "()=>{\n\tconst thing = useStore();\n\tconst {foo, bar} = foo();\n\n\treturn (\n\t\t<div>{thing}</div>\n\t);\n}";
        let result = eliminate_dead_code_in_body(body);
        eprintln!("INPUT: {:?}", body);
        eprintln!("OUTPUT: {:?}", result);
        // Should be identical - nothing to remove
        assert_eq!(body, result.as_str(), "DCE should not modify this body");
    }
}

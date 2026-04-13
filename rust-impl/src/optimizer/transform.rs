/// Public entry point for the Qwik optimizer.
///
/// transform_modules() accepts TransformModulesOptions and returns TransformOutput,
/// wiring together extraction, capture analysis, variable migration, parent
/// rewriting, and segment codegen.
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::hashing::naming::{build_display_name, build_symbol_name};
use crate::hashing::siphash::qwik_hash;

use super::entry_strategy::resolve_entry_field;
use super::extract::extract_segments;
use super::jsx_transform::reset_jsx_key_counter;
use super::marker_detection::collect_imports;
use super::rewrite_parent::rewrite_parent_module;
use super::segment_codegen::generate_segment_code;
use super::types::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Determine file extension from a path string.
fn get_extension(file_path: &str) -> &str {
    if let Some(dot_idx) = file_path.rfind('.') {
        &file_path[dot_idx..]
    } else {
        ""
    }
}

/// Normalize a path to use forward slashes.
fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// Compute relative path from src_dir.
fn compute_rel_path(input_path: &str, src_dir: &str) -> String {
    let norm_input = normalize_path(input_path);
    let norm_src = normalize_path(src_dir);

    if norm_src == "." || norm_src.is_empty() || norm_src == "./" {
        return norm_input;
    }

    let prefix = if norm_src.ends_with('/') {
        norm_src.clone()
    } else {
        format!("{}/", norm_src)
    };

    if norm_input.starts_with(&prefix) {
        norm_input[prefix.len()..].to_string()
    } else {
        norm_input
    }
}

/// Determine output extension based on transpile settings.
fn output_extension(
    input_ext: &str,
    transpile_ts: bool,
    transpile_jsx: bool,
) -> &'static str {
    match input_ext {
        ".tsx" => {
            if transpile_ts && transpile_jsx {
                ".js"
            } else if transpile_ts {
                ".jsx"
            } else if transpile_jsx {
                ".ts"
            } else {
                ".tsx"
            }
        }
        ".ts" => {
            if transpile_ts { ".js" } else { ".ts" }
        }
        ".jsx" => {
            if transpile_jsx { ".js" } else { ".jsx" }
        }
        _ => ".js",
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Transform a set of modules according to the Qwik optimizer rules.
pub fn transform_modules(options: &TransformModulesOptions) -> TransformOutput {
    let mut all_modules = Vec::new();
    let mut all_diagnostics = Vec::new();
    let mut is_type_script = false;
    let mut is_jsx = false;

    for input in &options.input {
        let result = transform_single_module(input, options);
        all_modules.extend(result.modules);
        all_diagnostics.extend(result.diagnostics);
        is_type_script = is_type_script || result.is_type_script;
        is_jsx = is_jsx || result.is_jsx;
    }

    TransformOutput {
        modules: all_modules,
        diagnostics: all_diagnostics,
        is_type_script,
        is_jsx,
    }
}

/// Transform a single input module.
fn transform_single_module(
    input: &TransformModuleInput,
    options: &TransformModulesOptions,
) -> TransformOutput {
    let file_path = &input.path;
    let source = &input.code;
    let src_dir = &options.src_dir;

    let ext = get_extension(file_path);
    let is_ts = ext == ".ts" || ext == ".tsx";
    let is_jsx_file = ext == ".jsx" || ext == ".tsx";
    let transpile_ts = options.transpile_ts.unwrap_or(true);
    let transpile_jsx = options.transpile_jsx.unwrap_or(true);
    let explicit_extensions = options.explicit_extensions.unwrap_or(false);

    let rel_path = compute_rel_path(file_path, src_dir);

    // Determine source type for OXC parser
    let source_type = match ext {
        ".tsx" => SourceType::tsx(),
        ".ts" => SourceType::ts(),
        ".jsx" => SourceType::jsx(),
        _ => SourceType::mjs(),
    };

    // Phase 1: Parse
    let allocator = Allocator::default();
    let parse_result = Parser::new(&allocator, source, source_type).parse();

    // If parser panicked, try to recover — but also check for non-fatal errors
    // OXC parser is recoverable, so even with errors the program may be usable
    // Note: some inputs have parse errors — OXC parser is recoverable,
    // so we proceed with whatever AST was produced.
    if parse_result.panicked && parse_result.program.body.is_empty() {
        return TransformOutput {
            modules: vec![TransformModule {
                path: rel_path,
                is_entry: false,
                code: source.to_string(),
                map: None,
                segment: None,
                orig_path: Some(file_path.to_string()),
            }],
            diagnostics: vec![],
            is_type_script: is_ts,
            is_jsx: is_jsx_file,
        };
    }

    let program = &parse_result.program;

    // Phase 2: Collect imports
    let imports = collect_imports(program);

    // Phase 3: Extract segments
    let extractions = extract_segments(source, program, &imports, file_path, transpile_jsx);

    if extractions.is_empty() {
        // No segments to extract - return as-is
        let out_ext = output_extension(ext, transpile_ts, transpile_jsx);
        let out_path = replace_extension(&rel_path, out_ext);

        return TransformOutput {
            modules: vec![TransformModule {
                path: out_path,
                is_entry: false,
                code: source.to_string(),
                map: None,
                segment: None,
                orig_path: Some(file_path.to_string()),
            }],
            diagnostics: vec![],
            is_type_script: is_ts,
            is_jsx: is_jsx_file,
        };
    }

    // Compute JSX key prefix: first 2 chars of base64(SipHash(scope + rel_path))
    let jsx_key_prefix = {
        let file_hash = crate::hashing::siphash::file_hash_base64(options.scope.as_deref(), &rel_path);
        if file_hash.len() >= 2 { file_hash[..2].to_string() } else { file_hash }
    };

    // Set key prefix on all extractions
    let mut extractions = extractions;
    for ext_item in &mut extractions {
        ext_item.jsx_key_prefix = Some(jsx_key_prefix.clone());
    }

    // Phase 4: Build display names and symbol names
    // SWC uses basename (e.g., "test.tsx") for display names, full rel_path for hashing
    let file_name = rel_path.rsplit('/').next().unwrap_or(&rel_path);
    let file_stem = file_name; // e.g., "test.tsx" or "lib.mjs"
    let mut symbol_names = Vec::new();
    let mut display_names = Vec::new();
    let mut canonical_filenames = Vec::new();
    let mut segment_analyses = Vec::new();

    let scope = options.scope.as_deref();
    let out_ext_str = output_extension(ext, transpile_ts, transpile_jsx);

    // Track display name counts for deduplication (SWC appends _1, _2, etc.)
    let mut display_name_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for extraction in &extractions {
        let mut display_name = if let Some(ref override_name) = extraction.display_name_override {
            // Use import-derived display name (e.g., "serverless_handler")
            format!("{}_{}", file_stem, override_name)
        } else {
            build_display_name(file_stem, &extraction.context_stack.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        };

        // Handle duplicate display names by appending _N suffix
        let context_portion_base = display_name[file_stem.len() + 1..].to_string();
        let count = display_name_counts.entry(context_portion_base.clone()).or_insert(0);
        if *count > 0 {
            display_name = format!("{}_{}", display_name, count);
        }
        *display_name_counts.get_mut(&context_portion_base).unwrap() += 1;

        let context_portion = &display_name[file_stem.len() + 1..];

        // For import-based QRL args, hash_seed_override replaces normal hash inputs
        let hash = if let Some(ref seed) = extraction.hash_seed_override {
            qwik_hash(None, seed, "")
        } else {
            qwik_hash(scope, &rel_path, context_portion)
        };
        let symbol_name = if extraction.hash_seed_override.is_some() {
            // For import-based args, symbol name uses the override context + hash
            format!("{}_{}", context_portion, hash)
        } else {
            build_symbol_name(&display_name, scope, &rel_path)
        };
        let canonical_filename = format!("{}_{}", file_stem, &symbol_name);

        let segment_ext = if transpile_ts && transpile_jsx {
            "js"
        } else if transpile_ts {
            if is_jsx_file { "jsx" } else { "js" }
        } else if transpile_jsx {
            if is_ts { "ts" } else { "js" }
        } else {
            ext.trim_start_matches('.')
        };

        let analysis = SegmentAnalysis {
            origin: rel_path.clone(),
            name: symbol_name.clone(),
            entry: resolve_entry_field(&options.entry_strategy, &symbol_name, &extraction.marker_name),
            display_name: display_name.clone(),
            hash: hash.clone(),
            canonical_filename: canonical_filename.clone(),
            extension: segment_ext.to_string(),
            parent: extraction.parent_segment.as_ref().and_then(|ps| {
                // Resolve "segment_N" to the actual symbol name of the parent
                if let Some(idx_str) = ps.strip_prefix("segment_") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        symbol_names.get(idx).cloned()
                    } else {
                        Some(ps.clone())
                    }
                } else {
                    Some(ps.clone())
                }
            }),
            ctx_kind: extraction.ctx_kind,
            ctx_name: extraction.marker_name.clone(),
            captures: false, // TODO: update after capture analysis
            loc: (extraction.start, extraction.end),
        };

        symbol_names.push(symbol_name);
        display_names.push(display_name);
        canonical_filenames.push(canonical_filename);
        segment_analyses.push(analysis);
    }

    // Phase 5: Capture analysis
    // Note: proper capture analysis requires tracking function-level scope
    // (variables declared inside component bodies, not module-level).
    // Module-level vars are NOT captures — they're imported by segments.
    // For now, we only detect captures for nested segments whose parent
    // body has local variable declarations.
    let import_names: Vec<String> = imports.iter().map(|i| i.local_name.clone()).collect();
    let module_var_names: Vec<String> = collect_module_var_names(program);

    // All module-level names should be excluded from captures
    let mut non_capture_names: Vec<String> = module_var_names.clone();
    non_capture_names.extend(import_names.clone());

    // For each extraction, compute ALL captured vars (both component-level and iteration-level).
    // Process sequentially so that child segments can include their parent's captures
    // in the available scope (important for grandchild segments like event handlers
    // inside $() closures inside component$ bodies).
    let mut all_captured: Vec<Vec<String>> = vec![Vec::new(); extractions.len()];
    for (i, e) in extractions.iter().enumerate() {
        if let Some(ref parent_key) = e.parent_segment {
            let parent_idx = parent_key
                .strip_prefix("segment_")
                .and_then(|s| s.parse::<usize>().ok());
            let mut parent_scope_vars: Vec<String> = if let Some(idx) = parent_idx {
                if idx < extractions.len() {
                    super::segment_codegen::collect_scope_bindings_deep(&extractions[idx].body_text)
                        .unwrap_or_else(|| {
                            super::segment_codegen::collect_local_declarations(&extractions[idx].body_text)
                        })
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            // Also include the parent segment's own captures, so grandchild
            // segments can see variables that the parent captures from its
            // grandparent (e.g., event handler inside $() inside component$).
            if let Some(idx) = parent_idx {
                if idx < all_captured.len() {
                    parent_scope_vars.extend(all_captured[idx].iter().cloned());
                }
            }
            let result = super::capture_analysis::analyze_captures(
                &e.body_text,
                &parent_scope_vars,
                &non_capture_names,
            );
            all_captured[i] = result.captured_vars;
        }
    }

    // _rawProps capture remapping: when a parent component$ has destructured props,
    // child segments should capture `_rawProps` instead of individual destructured fields.
    // E.g., parent `({foo, bar}) => { ... }` → child captures `_rawProps` not `foo`/`bar`.
    for i in 0..extractions.len() {
        if all_captured[i].is_empty() {
            continue;
        }
        if let Some(ref parent_key) = extractions[i].parent_segment {
            let parent_idx = parent_key
                .strip_prefix("segment_")
                .and_then(|s| s.parse::<usize>().ok());
            if let Some(idx) = parent_idx {
                if idx < extractions.len()
                    && extractions[idx].has_destructured_props
                    && (extractions[idx].marker_name == "component$"
                        || extractions[idx].marker_name.ends_with("component$"))
                {
                    // Get the destructured param names from the parent
                    let destructured_names = collect_destructured_param_names(&extractions[idx].body_text);
                    if !destructured_names.is_empty() {
                        // Replace any captured var that matches a destructured name with `_rawProps`.
                        // _rawProps goes first (matching SWC: props param position comes first),
                        // then other captures in their original order.
                        let mut other_captures: Vec<String> = Vec::new();
                        let mut needs_raw_props = false;
                        for cap in &all_captured[i] {
                            if destructured_names.contains(cap.as_str()) {
                                needs_raw_props = true;
                            } else {
                                other_captures.push(cap.clone());
                            }
                        }
                        if needs_raw_props {
                            let mut new_captures = vec!["_rawProps".to_string()];
                            new_captures.extend(other_captures);
                            all_captured[i] = new_captures;
                        }
                    }
                }
            }
        }
    }

    // Split captures: for event handlers, iteration vars become positional params
    // (passed via q:p/q:ps in the parent JSX), not _captures entries.
    let iter_var_sets: Vec<std::collections::HashSet<String>> = extractions
        .iter()
        .map(|e| e.iteration_vars.iter().cloned().collect())
        .collect();

    // "captures" = component-level captures only (go into _captures / .w([]))
    let captures: Vec<Vec<String>> = all_captured.iter().enumerate()
        .map(|(i, caps)| {
            if extractions[i].ctx_kind == super::types::SegmentKind::EventHandler && !iter_var_sets[i].is_empty() {
                caps.iter()
                    .filter(|c| !iter_var_sets[i].contains(c.as_str()))
                    .cloned()
                    .collect()
            } else {
                caps.clone()
            }
        })
        .collect();

    // Compute which iteration vars each handler actually references.
    let handler_used_vars: Vec<std::collections::HashSet<String>> = extractions.iter().enumerate()
        .map(|(i, e)| {
            if e.ctx_kind == super::types::SegmentKind::EventHandler && !iter_var_sets[i].is_empty() {
                let referenced = super::capture_analysis::collect_body_references(&e.body_text);
                e.iteration_vars.iter()
                    .filter(|v| referenced.contains(v.as_str()))
                    .cloned()
                    .collect()
            } else {
                std::collections::HashSet::new()
            }
        })
        .collect();

    // Group sibling event handlers that share the same parent element.
    // Handlers on the same element share the same parent_segment and iteration_vars
    // (they're in the same loop/callback scope and same JSX element).
    // The unified var list for the group determines positional params.
    // Build a map: group_key → sorted union of used vars
    // group_key → list of handler indices in the group
    let mut element_group_indices: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, e) in extractions.iter().enumerate() {
        if e.ctx_kind == super::types::SegmentKind::EventHandler && !iter_var_sets[i].is_empty() {
            let ctx_prefix = if e.context_stack.len() >= 2 {
                e.context_stack[..e.context_stack.len() - 1].join("/")
            } else {
                e.context_stack.join("/")
            };
            let group_key = format!("{}::{}", e.parent_segment.as_deref().unwrap_or(""), ctx_prefix);
            element_group_indices.entry(group_key).or_default().push(i);
        }
    }

    // Build unified var list per group: iterate iteration_vars in declaration order,
    // include any var used by ANY handler in the group.
    let mut element_groups: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (key, indices) in &element_group_indices {
        // Use the iteration_vars from the first handler (they share the same scope)
        if let Some(&first_idx) = indices.first() {
            let iter_vars = &extractions[first_idx].iteration_vars;
            let unified: Vec<String> = iter_vars.iter()
                .filter(|v| indices.iter().any(|&idx| handler_used_vars[idx].contains(v.as_str())))
                .cloned()
                .collect();
            element_groups.insert(key.clone(), unified);
        }
    }

    // Build iteration_captures for each handler using the element group's unified var list.
    let iteration_captures: Vec<Vec<String>> = extractions.iter().enumerate()
        .map(|(i, e)| {
            if e.ctx_kind == super::types::SegmentKind::EventHandler && !iter_var_sets[i].is_empty() {
                let ctx_prefix = if e.context_stack.len() >= 2 {
                    e.context_stack[..e.context_stack.len() - 1].join("/")
                } else {
                    e.context_stack.join("/")
                };
                let group_key = format!("{}::{}", e.parent_segment.as_deref().unwrap_or(""), ctx_prefix);
                if let Some(unified_vars) = element_groups.get(&group_key) {
                    if unified_vars.is_empty() {
                        return Vec::new();
                    }
                    // Build positional param list with _N placeholders for unused positions
                    let mut placeholder_idx = 2u32;
                    let full: Vec<(String, bool)> = unified_vars.iter()
                        .map(|v| {
                            let used = handler_used_vars[i].contains(v);
                            let name = if used { v.clone() } else { format!("_{}", placeholder_idx) };
                            placeholder_idx += 1;
                            (name, used)
                        })
                        .collect();
                    // Trim trailing unused placeholders
                    let last_used = full.iter().rposition(|(_, used)| *used);
                    match last_used {
                        Some(idx) => full[..=idx].iter().map(|(n, _)| n.clone()).collect(),
                        None => Vec::new(),
                    }
                } else {
                    handler_used_vars[i].iter().cloned().collect()
                }
            } else {
                Vec::new()
            }
        })
        .collect();

    // Update segment analyses with capture info
    // captures=true only if there are component-level captures (not just iteration params)
    for (i, cap) in captures.iter().enumerate() {
        if !cap.is_empty() {
            segment_analyses[i].captures = true;
        }
    }

    // Phase 6: Generate segment modules
    // Compute the file stem for segment imports from parent.
    // When explicit_extensions is enabled, preserve the full file name (e.g., "test.tsx")
    // so segment imports like `import { foo } from "./test.tsx"` match SWC output.
    // Otherwise strip the extension: "test.tsx" -> "test", "lib.mjs" -> "lib"
    let parent_file_stem_no_ext = if explicit_extensions {
        file_stem
    } else if let Some(dot_idx) = file_stem.rfind('.') {
        &file_stem[..dot_idx]
    } else {
        file_stem
    };

    let mut modules = Vec::new();
    let seg_exts: Vec<String> = segment_analyses.iter().map(|s| s.extension.clone()).collect();

    // Reset JSX key counter once per file so keys increment across segments
    reset_jsx_key_counter();

    for (i, extraction) in extractions.iter().enumerate() {
        // Compute props_remap: if this segment's parent is a component$ with destructured props,
        // collect the destructured field names so the segment body can replace `foo` with `_rawProps.foo`
        let props_remap: std::collections::HashSet<String> = if let Some(ref parent_key) = extraction.parent_segment {
            let parent_idx = parent_key
                .strip_prefix("segment_")
                .and_then(|s| s.parse::<usize>().ok());
            if let Some(idx) = parent_idx {
                if idx < extractions.len()
                    && extractions[idx].has_destructured_props
                    && (extractions[idx].marker_name == "component$"
                        || extractions[idx].marker_name.ends_with("component$"))
                {
                    collect_destructured_param_names(&extractions[idx].body_text)
                } else {
                    std::collections::HashSet::new()
                }
            } else {
                std::collections::HashSet::new()
            }
        } else {
            std::collections::HashSet::new()
        };

        let segment_code = generate_segment_code(
            extraction,
            &symbol_names[i],
            &imports,
            &captures[i],
            &[], // unused legacy param
            parent_file_stem_no_ext,
            &module_var_names,
            transpile_jsx,
            transpile_ts,
            i,
            &extractions,
            &symbol_names,
            &canonical_filenames,
            &captures,
            &seg_exts,
            explicit_extensions,
            &iteration_captures[i],
            &props_remap,
        );

        let segment_path = format!("{}.{}", canonical_filenames[i], segment_analyses[i].extension);

        modules.push(TransformModule {
            path: segment_path,
            is_entry: true,
            code: segment_code,
            map: None, // TODO: source maps
            segment: Some(segment_analyses[i].clone()),
            orig_path: None,
        });
    }

    // Phase 7: Rewrite parent module
    let parent_result = rewrite_parent_module(
        source,
        &extractions,
        &symbol_names,
        &canonical_filenames,
        &captures,
        &imports,
        file_path,
        &seg_exts,
        explicit_extensions,
    );

    let parent_path = replace_extension(&rel_path, out_ext_str);

    // Parent module goes first
    modules.insert(0, TransformModule {
        path: parent_path,
        is_entry: false,
        code: parent_result.code,
        map: parent_result.source_map,
        segment: None,
        orig_path: Some(file_path.to_string()),
    });

    TransformOutput {
        modules,
        diagnostics: vec![],
        is_type_script: is_ts,
        is_jsx: is_jsx_file,
    }
}

/// Collect all module-level variable/function/class declaration names.
/// These are the potential parent scope variables for capture analysis.
fn collect_module_var_names(program: &oxc_ast::ast::Program) -> Vec<String> {
    use oxc_ast::ast::*;
    let mut names = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                        names.push(id.name.to_string());
                    }
                }
            }
            Statement::FunctionDeclaration(fn_decl) => {
                if let Some(ref id) = fn_decl.id {
                    names.push(id.name.to_string());
                }
            }
            Statement::ClassDeclaration(class_decl) => {
                if let Some(ref id) = class_decl.id {
                    names.push(id.name.to_string());
                }
            }
            Statement::TSEnumDeclaration(enum_decl) => {
                names.push(enum_decl.id.name.to_string());
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(ref decl) = export.declaration {
                    match decl {
                        Declaration::VariableDeclaration(var_decl) => {
                            for d in &var_decl.declarations {
                                if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                                    names.push(id.name.to_string());
                                }
                            }
                        }
                        Declaration::FunctionDeclaration(fn_decl) => {
                            if let Some(ref id) = fn_decl.id {
                                names.push(id.name.to_string());
                            }
                        }
                        Declaration::ClassDeclaration(class_decl) => {
                            if let Some(ref id) = class_decl.id {
                                names.push(id.name.to_string());
                            }
                        }
                        Declaration::TSEnumDeclaration(enum_decl) => {
                            names.push(enum_decl.id.name.to_string());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    names
}

/// Collect the top-level field names from a destructured object parameter.
/// Given a body like `({foo, bar: baz, ...rest}) => { ... }`, returns {"foo", "baz", "rest"}.
/// These are the local binding names introduced by the destructuring pattern.
fn collect_destructured_param_names(body_text: &str) -> std::collections::HashSet<String> {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let mut names = std::collections::HashSet::new();

    let wrapped = format!("const __qwik_fn = {};", body_text);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked {
        return names;
    }

    // Find the arrow/function expression
    for stmt in &parse_result.program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let Some(ref init) = declarator.init {
                    match init {
                        Expression::ArrowFunctionExpression(arrow) => {
                            if let Some(first_param) = arrow.params.items.first() {
                                collect_object_pattern_names(&first_param.pattern, &mut names);
                            }
                        }
                        Expression::FunctionExpression(fn_expr) => {
                            if let Some(first_param) = fn_expr.params.items.first() {
                                collect_object_pattern_names(&first_param.pattern, &mut names);
                            }
                        }
                        Expression::ParenthesizedExpression(paren) => {
                            if let Expression::ArrowFunctionExpression(arrow) = &paren.expression {
                                if let Some(first_param) = arrow.params.items.first() {
                                    collect_object_pattern_names(&first_param.pattern, &mut names);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    names
}

/// Recursively collect all binding names from an object destructuring pattern.
fn collect_object_pattern_names(
    pattern: &oxc_ast::ast::BindingPattern,
    names: &mut std::collections::HashSet<String>,
) {
    use oxc_ast::ast::*;
    match &pattern.kind {
        BindingPatternKind::ObjectPattern(obj) => {
            for prop in &obj.properties {
                // The value is the local binding (which may be another pattern)
                collect_object_pattern_names(&prop.value, names);
            }
            if let Some(ref rest) = obj.rest {
                collect_object_pattern_names(&rest.argument, names);
            }
        }
        BindingPatternKind::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_object_pattern_names(elem, names);
            }
            if let Some(ref rest) = arr.rest {
                collect_object_pattern_names(&rest.argument, names);
            }
        }
        BindingPatternKind::AssignmentPattern(assign) => {
            collect_object_pattern_names(&assign.left, names);
        }
        BindingPatternKind::BindingIdentifier(id) => {
            names.insert(id.name.to_string());
        }
    }
}

fn replace_extension(path: &str, new_ext: &str) -> String {
    if let Some(dot_idx) = path.rfind('.') {
        format!("{}{}", &path[..dot_idx], new_ext)
    } else {
        format!("{}{}", path, new_ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_rel_path() {
        assert_eq!(compute_rel_path("src/test.tsx", "src"), "test.tsx");
        assert_eq!(compute_rel_path("test.tsx", "."), "test.tsx");
    }

    #[test]
    fn test_get_extension() {
        assert_eq!(get_extension("foo.tsx"), ".tsx");
        assert_eq!(get_extension("bar.ts"), ".ts");
        assert_eq!(get_extension("baz"), "");
    }

    #[test]
    fn test_replace_extension() {
        assert_eq!(replace_extension("test.tsx", ".js"), "test.js");
        assert_eq!(replace_extension("test.tsx", ".ts"), "test.ts");
    }

    #[test]
    fn test_transform_empty_module() {
        let options = TransformModulesOptions {
            input: vec![TransformModuleInput {
                path: "test.tsx".to_string(),
                code: "const x = 1;".to_string(),
                dev_path: None,
            }],
            src_dir: ".".to_string(),
            root_dir: None,
            entry_strategy: None,
            minify: None,
            source_maps: None,
            transpile_ts: Some(true),
            transpile_jsx: Some(true),
            preserve_filenames: None,
            explicit_extensions: None,
            mode: None,
            scope: None,
            strip_exports: None,
            reg_ctx_name: None,
            strip_ctx_name: None,
            strip_event_handlers: None,
            is_server: None,
        };

        let output = transform_modules(&options);
        assert_eq!(output.modules.len(), 1);
        assert_eq!(output.modules[0].code, "const x = 1;");
    }

    #[test]
    fn test_transform_basic_qrl() {
        let code = r#"
import { $, component } from '@qwik.dev/core';

export const renderHeader1 = $(() => {
    return "hello";
});
"#;
        let options = TransformModulesOptions {
            input: vec![TransformModuleInput {
                path: "test.tsx".to_string(),
                code: code.to_string(),
                dev_path: None,
            }],
            src_dir: ".".to_string(),
            root_dir: None,
            entry_strategy: None,
            minify: None,
            source_maps: None,
            transpile_ts: Some(true),
            transpile_jsx: Some(true),
            preserve_filenames: None,
            explicit_extensions: None,
            mode: None,
            scope: None,
            strip_exports: None,
            reg_ctx_name: None,
            strip_ctx_name: None,
            strip_event_handlers: None,
            is_server: None,
        };

        let output = transform_modules(&options);
        // Should have parent + 1 segment
        assert!(output.modules.len() >= 2, "Expected at least 2 modules, got {}", output.modules.len());

        // Check segment has correct symbol name
        let segment = &output.modules[1];
        assert!(segment.segment.is_some());
        let analysis = segment.segment.as_ref().unwrap();
        assert_eq!(analysis.name, "renderHeader1_jMxQsjbyDss");
        assert_eq!(analysis.hash, "jMxQsjbyDss");
    }

    #[test]
    fn test_transform_component_with_nested_handler() {
        let code = r#"
import { component$ } from '@qwik.dev/core';

export const App = component$(() => {
    return <div onClick$={() => console.log('hi')}>Hello</div>;
});
"#;
        let options = TransformModulesOptions {
            input: vec![TransformModuleInput {
                path: "test.tsx".to_string(),
                code: code.to_string(),
                dev_path: None,
            }],
            src_dir: ".".to_string(),
            root_dir: None,
            entry_strategy: None,
            minify: None,
            source_maps: None,
            transpile_ts: Some(true),
            transpile_jsx: Some(true),
            preserve_filenames: None,
            explicit_extensions: None,
            mode: None,
            scope: None,
            strip_exports: None,
            reg_ctx_name: None,
            strip_ctx_name: None,
            strip_event_handlers: None,
            is_server: None,
        };

        let output = transform_modules(&options);
        let segments: Vec<_> = output.modules.iter()
            .filter_map(|m| m.segment.as_ref())
            .collect();

        eprintln!("Nested test segments:");
        for s in &segments {
            eprintln!("  name={}, dn={}, hash={}", s.name, s.display_name, s.hash);
        }

        // Should have at least 2 segments: component$ body + onClick$ handler
        assert!(segments.len() >= 2, "Expected at least 2 segments, got {}", segments.len());

        // Check the component segment
        let comp = segments.iter().find(|s| s.display_name.contains("component"));
        assert!(comp.is_some(), "Should find component segment");

        // Check the onClick handler segment
        let handler = segments.iter().find(|s| s.display_name.contains("q_e_click") || s.display_name.contains("onClick"));
        assert!(handler.is_some(), "Should find onClick handler segment. Segments: {:?}", segments.iter().map(|s| &s.display_name).collect::<Vec<_>>());
    }

    #[test]
    fn test_transform_multiple_event_handlers() {
        let code = r#"
import { component$, useSignal, $ } from "@qwik.dev/core";

export const Test = component$(() => {
    const sig = useSignal(0);
    return <button onClick$={() => sig.value++} onDblClick$={() => sig.value--}>click</button>;
});
"#;
        let options = TransformModulesOptions {
            input: vec![TransformModuleInput {
                path: "test.tsx".to_string(),
                code: code.to_string(),
                dev_path: None,
            }],
            src_dir: ".".to_string(),
            root_dir: None,
            entry_strategy: None,
            minify: None,
            source_maps: None,
            transpile_ts: Some(true),
            transpile_jsx: Some(true),
            preserve_filenames: None,
            explicit_extensions: None,
            mode: None,
            scope: None,
            strip_exports: None,
            reg_ctx_name: None,
            strip_ctx_name: None,
            strip_event_handlers: None,
            is_server: None,
        };

        let output = transform_modules(&options);
        let segments: Vec<_> = output.modules.iter()
            .filter_map(|m| m.segment.as_ref())
            .collect();

        eprintln!("Multi-handler segments:");
        for s in &segments {
            eprintln!("  name={}, dn={}", s.name, s.display_name);
        }

        // Should have component$ + onClick$ + onDblClick$ = 3 segments
        assert!(segments.len() >= 3, "Expected at least 3 segments, got {}. Segments: {:?}",
            segments.len(), segments.iter().map(|s| &s.display_name).collect::<Vec<_>>());

        // Verify dblclick handler
        let dblclick = segments.iter().find(|s| s.display_name.contains("dblclick"));
        assert!(dblclick.is_some(), "Should find dblclick handler. Segments: {:?}",
            segments.iter().map(|s| &s.display_name).collect::<Vec<_>>());
    }
}

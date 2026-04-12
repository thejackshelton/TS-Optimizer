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
            parent: extraction.parent_segment.clone(),
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

    let captures: Vec<Vec<String>> = extractions
        .iter()
        .map(|e| {
            // Only analyze captures for segments that have a parent (nested segments)
            if e.parent_segment.is_some() {
                // The parent body's local vars would be the capture candidates
                // For now, we use a simplified approach: if the segment body references
                // variables not in module scope or imports, they're captures
                let result = super::capture_analysis::analyze_captures(
                    &e.body_text,
                    &[], // TODO: pass actual parent scope vars
                    &non_capture_names,
                );
                result.captured_vars
            } else {
                Vec::new()
            }
        })
        .collect();

    // Update segment analyses with capture info
    for (i, cap) in captures.iter().enumerate() {
        if !cap.is_empty() {
            segment_analyses[i].captures = true;
        }
    }

    // Phase 6: Generate segment modules
    let mut modules = Vec::new();

    for (i, extraction) in extractions.iter().enumerate() {
        let segment_code = generate_segment_code(
            extraction,
            &symbol_names[i],
            &imports,
            &captures[i],
            &[], // TODO: nested QRL decls
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
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    names
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

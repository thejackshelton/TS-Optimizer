/// Identifies $-suffixed CallExpression nodes that should trigger extraction.
///
/// Collects imports and detects custom inlined functions from @qwik.dev/core.
use std::collections::HashMap;

use oxc_ast::ast::*;

/// Information about an import from the source module.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub source: String,
    pub specifier: String,
    pub local_name: String,
    pub is_type: bool,
}

/// Known Qwik marker function suffixes that trigger QRL extraction.
pub const MARKER_SUFFIX: &str = "$";

/// Known Qwik core functions that are QRL markers.
pub const QWIK_CORE_MARKERS: &[&str] = &[
    "component$",
    "event$",
    "sync$",
    "$",
];

/// Map of $ functions to their Qrl counterparts.
pub fn dollar_to_qrl_name(name: &str) -> Option<&'static str> {
    match name {
        "component$" => Some("componentQrl"),
        "event$" => Some("eventQrl"),
        "$" => Some("qrl"),
        _ => {
            // Generic: foo$ → fooQrl
            if name.ends_with('$') {
                None // handled dynamically
            } else {
                None
            }
        }
    }
}

/// Convert a $-suffixed function name to its Qrl equivalent.
/// e.g., "component$" → "componentQrl", "globalAction$" → "globalActionQrl"
pub fn to_qrl_name(name: &str) -> String {
    if name == "$" {
        return "qrl".to_string();
    }
    if let Some(base) = name.strip_suffix('$') {
        format!("{}Qrl", base)
    } else {
        name.to_string()
    }
}

/// Check if a function name is a QRL marker (ends with $).
pub fn is_marker_function(name: &str) -> bool {
    name.ends_with('$') && !name.starts_with('_')
}

/// Collect all imports from a module's AST.
pub fn collect_imports(program: &Program) -> Vec<ImportInfo> {
    let mut imports = Vec::new();

    for item in &program.body {
        if let Statement::ImportDeclaration(decl) = item {
            let source = decl.source.value.as_str().to_string();
            let is_type = decl.import_kind.is_type();

            if let Some(specifiers) = &decl.specifiers {
                for spec in specifiers {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            imports.push(ImportInfo {
                                source: source.clone(),
                                specifier: s.imported.name().to_string(),
                                local_name: s.local.name.to_string(),
                                is_type: is_type || s.import_kind.is_type(),
                            });
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            imports.push(ImportInfo {
                                source: source.clone(),
                                specifier: "default".to_string(),
                                local_name: s.local.name.to_string(),
                                is_type,
                            });
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            imports.push(ImportInfo {
                                source: source.clone(),
                                specifier: "*".to_string(),
                                local_name: s.local.name.to_string(),
                                is_type,
                            });
                        }
                    }
                }
            }
        }
    }

    imports
}

/// Find which imported names are $ markers.
pub fn find_marker_imports(imports: &[ImportInfo]) -> HashMap<String, String> {
    let mut markers = HashMap::new();
    for imp in imports {
        if is_marker_function(&imp.specifier) {
            markers.insert(imp.local_name.clone(), imp.specifier.clone());
        }
    }
    markers
}

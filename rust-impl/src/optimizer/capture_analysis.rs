/// Detects variables crossing $() boundaries (serialization boundaries).
///
/// Identifies which variables must be captured via _captures injection.
/// A variable is "captured" if it's:
/// 1. Referenced inside the $() body
/// 2. Declared in an outer scope (parent function/component)
/// 3. NOT a module-level import or export
use std::collections::HashSet;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Result of capture analysis for a segment.
#[derive(Debug, Clone)]
pub struct CaptureAnalysisResult {
    /// Variables that need to be captured (cross the $() boundary)
    pub captured_vars: Vec<String>,
    /// Whether any captures exist
    pub has_captures: bool,
}

/// Analyze captures for a segment body given the parent scope declarations.
///
/// `body_text` is the source text of the extracted function/arrow expression.
/// `parent_scope_vars` are variable names declared in the parent scope.
/// `module_imports` are names imported at the module level.
pub fn analyze_captures(
    body_text: &str,
    parent_scope_vars: &[String],
    module_imports: &[String],
) -> CaptureAnalysisResult {
    // Parse the body to find all referenced identifiers
    let referenced = collect_referenced_idents(body_text);

    // Filter: only keep identifiers that are in parent scope but NOT module imports
    let parent_set: HashSet<&str> = parent_scope_vars.iter().map(|s| s.as_str()).collect();
    let import_set: HashSet<&str> = module_imports.iter().map(|s| s.as_str()).collect();

    let mut captured: Vec<String> = referenced
        .iter()
        .filter(|name| parent_set.contains(name.as_str()) && !import_set.contains(name.as_str()))
        .cloned()
        .collect();

    // Sort for deterministic output (matches SWC behavior)
    captured.sort();
    captured.dedup();

    let has_captures = !captured.is_empty();
    CaptureAnalysisResult {
        captured_vars: captured,
        has_captures,
    }
}

/// Public wrapper: collect all identifier references from a segment body.
/// Used by transform.rs to determine which iteration vars are actually used.
pub fn collect_body_references(code: &str) -> HashSet<String> {
    collect_referenced_idents(code)
}

/// Collect all identifier references in a code fragment.
/// Parses the code and walks the AST to find all Identifier nodes
/// that are references (not declarations).
fn collect_referenced_idents(code: &str) -> HashSet<String> {
    let mut refs = HashSet::new();

    // Try to parse as an expression first, wrapped in a function
    let wrapped = format!("const __qwik_capture_fn = {};", code);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked {
        return refs;
    }

    // Walk the AST to collect identifier references
    collect_idents_from_program(&parse_result.program, &mut refs);

    // Remove common globals that shouldn't be captured
    let globals = [
        "undefined", "null", "true", "false", "NaN", "Infinity",
        "console", "window", "document", "globalThis", "self",
        "setTimeout", "setInterval", "clearTimeout", "clearInterval",
        "Promise", "Array", "Object", "String", "Number", "Boolean",
        "Math", "Date", "JSON", "RegExp", "Error", "Map", "Set",
        "WeakMap", "WeakSet", "Symbol", "BigInt", "parseInt", "parseFloat",
        "isNaN", "isFinite", "encodeURI", "decodeURI",
        "encodeURIComponent", "decodeURIComponent",
        "fetch", "URL", "URLSearchParams", "Headers", "Request", "Response",
        "FormData", "Event", "CustomEvent", "AbortController",
        "__qwik_capture_fn",
    ];
    for g in &globals {
        refs.remove(*g);
    }

    refs
}

fn collect_idents_from_program(program: &Program, refs: &mut HashSet<String>) {
    // Track locally declared variables to exclude them
    let mut local_decls = HashSet::new();

    for stmt in &program.body {
        collect_idents_from_statement(stmt, refs, &mut local_decls);
    }

    // Remove locally declared variables from references
    for decl in &local_decls {
        refs.remove(decl);
    }
}

fn collect_idents_from_statement(
    stmt: &Statement,
    refs: &mut HashSet<String>,
    local_decls: &mut HashSet<String>,
) {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                collect_binding_names(&declarator.id, local_decls);
                if let Some(ref init) = declarator.init {
                    collect_idents_from_expr(init, refs);
                }
            }
        }
        Statement::ExpressionStatement(expr) => {
            collect_idents_from_expr(&expr.expression, refs);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                collect_idents_from_expr(arg, refs);
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_idents_from_expr(&if_stmt.test, refs);
            collect_idents_from_statement(&if_stmt.consequent, refs, local_decls);
            if let Some(ref alt) = if_stmt.alternate {
                collect_idents_from_statement(alt, refs, local_decls);
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_idents_from_statement(s, refs, local_decls);
            }
        }
        Statement::ForStatement(for_stmt) => {
            collect_idents_from_statement(&for_stmt.body, refs, local_decls);
        }
        Statement::ForInStatement(for_in) => {
            collect_idents_from_statement(&for_in.body, refs, local_decls);
        }
        Statement::ForOfStatement(for_of) => {
            collect_idents_from_statement(&for_of.body, refs, local_decls);
        }
        _ => {}
    }
}

fn collect_idents_from_expr(expr: &Expression, refs: &mut HashSet<String>) {
    match expr {
        Expression::Identifier(id) => {
            refs.insert(id.name.to_string());
        }
        Expression::CallExpression(call) => {
            collect_idents_from_expr(&call.callee, refs);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_idents_from_expr(e, refs);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_idents_from_expr(&member.object, refs);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_idents_from_expr(&member.object, refs);
            collect_idents_from_expr(&member.expression, refs);
        }
        Expression::ArrowFunctionExpression(arrow) => {
            // Parameters and local declarations are local to the arrow — don't collect as refs.
            // Accumulate inner_decls across ALL statements so that a variable declared
            // in one statement (e.g., `const handler = ...`) is recognized as local when
            // referenced in a later statement (e.g., `return <div onKeyup$={handler}/>`).
            let mut inner_decls = HashSet::new();
            for p in &arrow.params.items {
                collect_binding_names(&p.pattern, &mut inner_decls);
            }
            for s in &arrow.body.statements {
                collect_idents_from_statement(s, refs, &mut inner_decls);
            }
            for d in &inner_decls {
                refs.remove(d);
            }
        }
        Expression::FunctionExpression(fn_expr) => {
            if let Some(ref body) = fn_expr.body {
                let mut inner_decls = HashSet::new();
                for p in &fn_expr.params.items {
                    collect_binding_names(&p.pattern, &mut inner_decls);
                }
                for s in &body.statements {
                    collect_idents_from_statement(s, refs, &mut inner_decls);
                }
                for d in &inner_decls {
                    refs.remove(d);
                }
            }
        }
        Expression::BinaryExpression(bin) => {
            collect_idents_from_expr(&bin.left, refs);
            collect_idents_from_expr(&bin.right, refs);
        }
        Expression::LogicalExpression(logical) => {
            collect_idents_from_expr(&logical.left, refs);
            collect_idents_from_expr(&logical.right, refs);
        }
        Expression::UnaryExpression(unary) => {
            collect_idents_from_expr(&unary.argument, refs);
        }
        Expression::UpdateExpression(update) => {
            // argument is a SimpleAssignmentTarget — extract root identifier
            collect_idents_from_simple_target(&update.argument, refs);
        }
        Expression::ConditionalExpression(cond) => {
            collect_idents_from_expr(&cond.test, refs);
            collect_idents_from_expr(&cond.consequent, refs);
            collect_idents_from_expr(&cond.alternate, refs);
        }
        Expression::AssignmentExpression(assign) => {
            collect_idents_from_expr(&assign.right, refs);
            // Left side: extract identifier references from assignment target
            collect_idents_from_assignment_target(&assign.left, refs);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_idents_from_expr(e, refs);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_idents_from_expr(&paren.expression, refs);
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_idents_from_expr(e, refs);
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        collect_idents_from_expr(&p.value, refs);
                    }
                    ObjectPropertyKind::SpreadProperty(s) => {
                        collect_idents_from_expr(&s.argument, refs);
                    }
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(e) = elem.as_expression() {
                    collect_idents_from_expr(e, refs);
                }
            }
        }
        Expression::AwaitExpression(a) => {
            collect_idents_from_expr(&a.argument, refs);
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_idents_from_expr(&tagged.tag, refs);
        }
        Expression::NewExpression(new_expr) => {
            collect_idents_from_expr(&new_expr.callee, refs);
            for arg in &new_expr.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_idents_from_expr(e, refs);
                }
            }
        }
        Expression::JSXElement(jsx) => {
            collect_idents_from_jsx_element(jsx, refs);
        }
        Expression::JSXFragment(frag) => {
            for child in &frag.children {
                collect_idents_from_jsx_child(child, refs);
            }
        }
        Expression::YieldExpression(y) => {
            if let Some(ref arg) = y.argument {
                collect_idents_from_expr(arg, refs);
            }
        }
        _ => {}
    }
}

fn collect_idents_from_jsx_element(jsx: &JSXElement, refs: &mut HashSet<String>) {
    // Check if the tag name is a component (identifier reference)
    if let JSXElementName::Identifier(id) = &jsx.opening_element.name {
        let name = id.name.to_string();
        // Uppercase first letter = component reference, not an HTML tag
        if name.chars().next().map_or(false, |c| c.is_uppercase()) {
            refs.insert(name);
        }
    } else if let JSXElementName::IdentifierReference(id) = &jsx.opening_element.name {
        let name = id.name.to_string();
        if name.chars().next().map_or(false, |c| c.is_uppercase()) {
            refs.insert(name);
        }
    }
    // Collect from attributes
    for attr in &jsx.opening_element.attributes {
        match attr {
            JSXAttributeItem::Attribute(a) => {
                if let Some(ref val) = a.value {
                    match val {
                        JSXAttributeValue::ExpressionContainer(container) => {
                            if let Some(e) = container.expression.as_expression() {
                                collect_idents_from_expr(e, refs);
                            }
                        }
                        _ => {}
                    }
                }
            }
            JSXAttributeItem::SpreadAttribute(spread) => {
                collect_idents_from_expr(&spread.argument, refs);
            }
        }
    }
    // Collect from children
    for child in &jsx.children {
        collect_idents_from_jsx_child(child, refs);
    }
}

fn collect_idents_from_jsx_child(child: &JSXChild, refs: &mut HashSet<String>) {
    match child {
        JSXChild::Element(el) => {
            collect_idents_from_jsx_element(el, refs);
        }
        JSXChild::Fragment(frag) => {
            for c in &frag.children {
                collect_idents_from_jsx_child(c, refs);
            }
        }
        JSXChild::ExpressionContainer(container) => {
            if let Some(e) = container.expression.as_expression() {
                collect_idents_from_expr(e, refs);
            }
        }
        JSXChild::Spread(spread) => {
            collect_idents_from_expr(&spread.expression, refs);
        }
        _ => {} // Text, etc.
    }
}

/// Extract root identifier from a SimpleAssignmentTarget (used by update/assignment exprs).
/// For `state.count++`, extracts `state` from the member chain.
fn collect_idents_from_simple_target(target: &SimpleAssignmentTarget, refs: &mut HashSet<String>) {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
            refs.insert(id.name.to_string());
        }
        SimpleAssignmentTarget::StaticMemberExpression(member) => {
            collect_idents_from_expr(&member.object, refs);
        }
        SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            collect_idents_from_expr(&member.object, refs);
            collect_idents_from_expr(&member.expression, refs);
        }
        _ => {}
    }
}

fn collect_idents_from_assignment_target(target: &AssignmentTarget, refs: &mut HashSet<String>) {
    if let AssignmentTarget::AssignmentTargetIdentifier(id) = target {
        refs.insert(id.name.to_string());
    }
    // For other simple targets, extract root identifiers
    match target {
        AssignmentTarget::StaticMemberExpression(member) => {
            collect_idents_from_expr(&member.object, refs);
        }
        AssignmentTarget::ComputedMemberExpression(member) => {
            collect_idents_from_expr(&member.object, refs);
            collect_idents_from_expr(&member.expression, refs);
        }
        _ => {}
    }
}

fn collect_binding_names(pattern: &BindingPattern, names: &mut HashSet<String>) {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(id) => {
            names.insert(id.name.to_string());
        }
        BindingPatternKind::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_names(&prop.value, names);
            }
            if let Some(ref rest) = obj.rest {
                collect_binding_names(&rest.argument, names);
            }
        }
        BindingPatternKind::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_names(elem, names);
            }
            if let Some(ref rest) = arr.rest {
                collect_binding_names(&rest.argument, names);
            }
        }
        BindingPatternKind::AssignmentPattern(assign) => {
            collect_binding_names(&assign.left, names);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_captures() {
        let result = analyze_captures(
            "() => console.log('hello')",
            &[],
            &[],
        );
        assert!(!result.has_captures);
        assert!(result.captured_vars.is_empty());
    }

    #[test]
    fn test_captures_parent_var() {
        let result = analyze_captures(
            "() => console.log(state.count)",
            &["state".to_string()],
            &[],
        );
        assert!(result.has_captures);
        assert_eq!(result.captured_vars, vec!["state"]);
    }

    #[test]
    fn test_ignores_imports() {
        let result = analyze_captures(
            "() => useStore({count: 0})",
            &["useStore".to_string()],
            &["useStore".to_string()],
        );
        assert!(!result.has_captures);
    }

    #[test]
    fn test_multiple_captures() {
        let result = analyze_captures(
            "() => { foo.bar(); return baz; }",
            &["foo".to_string(), "baz".to_string(), "qux".to_string()],
            &[],
        );
        assert!(result.has_captures);
        assert_eq!(result.captured_vars, vec!["baz", "foo"]);
    }

    #[test]
    fn test_unused_parent_var_not_captured() {
        let result = analyze_captures(
            "() => { const y = 1; return y; }",
            &["x".to_string()],
            &[],
        );
        // x is in parent scope but not referenced in the body
        assert!(!result.has_captures);
    }

    #[test]
    fn test_captures_with_member_access() {
        let result = analyze_captures(
            "() => state.count++",
            &["state".to_string()],
            &[],
        );
        assert!(result.has_captures);
        assert_eq!(result.captured_vars, vec!["state"]);
    }
}

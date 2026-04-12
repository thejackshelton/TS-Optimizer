/// Core extraction engine.
///
/// Walks the AST to find marker calls ($-suffixed functions), extracts segment
/// info (body text, positions, metadata). Returns ExtractionResult objects.
use std::cell::Cell;

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::context_stack::ContextStack;
use super::marker_detection::{is_marker_function, ImportInfo};
use super::types::SegmentKind;

// Whether JSX is being transpiled (affects Fragment context behavior)
thread_local! {
    static TRANSPILE_JSX: Cell<bool> = const { Cell::new(false) };
}

// Store imports for display_name_override lookup (set during extract_segments)
thread_local! {
    static CURRENT_IMPORTS: std::cell::RefCell<Vec<ImportInfo>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Result of extracting a segment from the source.
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// The name of the marker function (e.g., "component$", "$", "onClick$")
    pub marker_name: String,
    /// Display name context elements (e.g., ["renderHeader1", "div", "onClick$"])
    pub context_stack: Vec<String>,
    /// The text of the segment body (the function/arrow expression)
    pub body_text: String,
    /// Start offset in the source
    pub start: u32,
    /// End offset in the source
    pub end: u32,
    /// Start offset of the call expression
    pub call_start: u32,
    /// End offset of the call expression
    pub call_end: u32,
    /// Whether this is an async function
    pub is_async: bool,
    /// Parameter names of the extracted function
    pub param_names: Vec<String>,
    /// Whether this segment is inside a JSX context
    pub in_jsx: bool,
    /// The kind of segment
    pub ctx_kind: SegmentKind,
    /// Parent segment name (if nested)
    pub parent_segment: Option<String>,
    /// Whether the extracted body contains JSX
    pub has_jsx: bool,
    /// Override display name (e.g., from import source for identifier arguments)
    pub display_name_override: Option<String>,
    /// Override hash seed (e.g., "source#specifier" for import-based QRL args)
    pub hash_seed_override: Option<String>,
}

/// Extract all segments from a source file.
///
/// Walks the AST looking for $-suffixed calls and extracts their arguments
/// as segments.
pub fn extract_segments(
    source: &str,
    program: &Program,
    imports: &[ImportInfo],
    file_path: &str,
    transpile_jsx: bool,
) -> Vec<ExtractionResult> {
    let mut results = Vec::new();
    let mut ctx = ContextStack::new();
    let marker_imports = super::marker_detection::find_marker_imports(imports);

    // Compute file stem for default export context
    let file_stem = compute_file_stem(file_path);

    // Store imports and transpile_jsx in thread-locals for nested access
    CURRENT_IMPORTS.with(|c| *c.borrow_mut() = imports.to_vec());
    TRANSPILE_JSX.set(transpile_jsx);

    extract_from_statements_with_defaults(source, &program.body, &mut results, &mut ctx, &marker_imports, None, &file_stem);

    // Deduplicate by (start, end) span — recursive walks may find the same segment twice
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert((r.start, r.end)))
;
    results
}

/// Compute file stem from path, stripping extension.
/// "test.tsx" → "test", "src/foo/index.tsx" → "index"
fn compute_file_stem(path: &str) -> String {
    let basename = path.rsplit('/').next().unwrap_or(path);
    basename.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(basename).to_string()
}

/// Top-level statement walker that handles export default with file stem context.
fn extract_from_statements_with_defaults(
    source: &str,
    stmts: &[Statement],
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
    file_stem: &str,
) {
    for stmt in stmts {
        if let Statement::ExportDefaultDeclaration(decl) = stmt {
            // For export default, push file stem as context (matching SWC behavior)
            ctx.push(file_stem);
            extract_from_expression_or_decl(source, &decl.declaration, results, ctx, marker_imports, parent_segment);
            ctx.pop();
        } else {
            extract_from_statement(source, stmt, results, ctx, marker_imports, parent_segment);
        }
    }
}

fn extract_from_statements(
    source: &str,
    stmts: &[Statement],
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    for stmt in stmts {
        extract_from_statement(source, stmt, results, ctx, marker_imports, parent_segment);
    }
}

fn extract_from_statement(
    source: &str,
    stmt: &Statement,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    match stmt {
        Statement::ExportNamedDeclaration(decl) => {
            if let Some(ref d) = decl.declaration {
                extract_from_declaration(source, d, results, ctx, marker_imports, parent_segment);
            }
        }
        Statement::ExportDefaultDeclaration(decl) => {
            // Non-top-level default exports (shouldn't happen, but handle gracefully)
            extract_from_expression_or_decl(source, &decl.declaration, results, ctx, marker_imports, parent_segment);
        }
        Statement::VariableDeclaration(decl) => {
            extract_from_var_decl(source, decl, results, ctx, marker_imports, parent_segment);
        }
        Statement::ExpressionStatement(expr_stmt) => {
            extract_from_expr(source, &expr_stmt.expression, results, ctx, marker_imports, parent_segment);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                extract_from_expr(source, arg, results, ctx, marker_imports, parent_segment);
            }
        }
        Statement::IfStatement(if_stmt) => {
            extract_from_expr(source, &if_stmt.test, results, ctx, marker_imports, parent_segment);
            extract_from_statement(source, &if_stmt.consequent, results, ctx, marker_imports, parent_segment);
            if let Some(ref alt) = if_stmt.alternate {
                extract_from_statement(source, alt, results, ctx, marker_imports, parent_segment);
            }
        }
        Statement::BlockStatement(block) => {
            extract_from_statements(source, &block.body, results, ctx, marker_imports, parent_segment);
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(ref update) = for_stmt.update {
                extract_from_expr(source, update, results, ctx, marker_imports, parent_segment);
            }
            extract_from_statement(source, &for_stmt.body, results, ctx, marker_imports, parent_segment);
        }
        Statement::ForInStatement(for_in) => {
            extract_from_statement(source, &for_in.body, results, ctx, marker_imports, parent_segment);
        }
        Statement::ForOfStatement(for_of) => {
            extract_from_statement(source, &for_of.body, results, ctx, marker_imports, parent_segment);
        }
        Statement::WhileStatement(while_stmt) => {
            extract_from_statement(source, &while_stmt.body, results, ctx, marker_imports, parent_segment);
        }
        Statement::DoWhileStatement(do_while) => {
            extract_from_statement(source, &do_while.body, results, ctx, marker_imports, parent_segment);
        }
        Statement::SwitchStatement(switch) => {
            for case in &switch.cases {
                extract_from_statements(source, &case.consequent, results, ctx, marker_imports, parent_segment);
            }
        }
        Statement::TryStatement(try_stmt) => {
            extract_from_statements(source, &try_stmt.block.body, results, ctx, marker_imports, parent_segment);
            if let Some(ref handler) = try_stmt.handler {
                extract_from_statements(source, &handler.body.body, results, ctx, marker_imports, parent_segment);
            }
            if let Some(ref finalizer) = try_stmt.finalizer {
                extract_from_statements(source, &finalizer.body, results, ctx, marker_imports, parent_segment);
            }
        }
        Statement::LabeledStatement(labeled) => {
            extract_from_statement(source, &labeled.body, results, ctx, marker_imports, parent_segment);
        }
        // Declaration variants inherited into Statement
        Statement::FunctionDeclaration(fn_decl) => {
            if let Some(ref id) = fn_decl.id {
                ctx.push(&id.name);
            }
            if let Some(ref body) = fn_decl.body {
                extract_from_statements(source, &body.statements, results, ctx, marker_imports, parent_segment);
            }
            if fn_decl.id.is_some() {
                ctx.pop();
            }
        }
        _ => {}
    }
}

fn extract_from_declaration(
    source: &str,
    decl: &Declaration,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    match decl {
        Declaration::VariableDeclaration(var_decl) => {
            extract_from_var_decl(source, var_decl, results, ctx, marker_imports, parent_segment);
        }
        Declaration::FunctionDeclaration(fn_decl) => {
            if let Some(ref id) = fn_decl.id {
                ctx.push(&id.name);
                // Walk function body for nested extractions
                if let Some(ref body) = fn_decl.body {
                    extract_from_statements(source, &body.statements, results, ctx, marker_imports, parent_segment);
                }
                ctx.pop();
            }
        }
        _ => {}
    }
}

fn extract_from_var_decl(
    source: &str,
    decl: &VariableDeclaration,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    for declarator in &decl.declarations {
        if let Some(ref init) = declarator.init {
            // Get the variable name for context
            let var_name = get_binding_name(&declarator.id);
            if let Some(ref name) = var_name {
                ctx.push(name);
            }

            extract_from_expr(source, init, results, ctx, marker_imports, parent_segment);

            if var_name.is_some() {
                ctx.pop();
            }
        }
    }
}

fn extract_from_expr(
    source: &str,
    expr: &Expression,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    match expr {
        Expression::CallExpression(call) => {
            // Check if callee is a marker function
            if let Some(marker_name) = get_call_marker_name(call, marker_imports) {
                // Get the local callee name for context (what the user wrote, not the specifier).
                // e.g., `import { component$ as Component }` → push "Component"
                let local_callee = get_local_callee_name(&call.callee)
                    .unwrap_or_else(|| marker_name.clone());

                // For non-bare $ markers, push the local callee name onto context stack.
                // escape_sym will strip the $. For bare $ (even if renamed), don't push.
                // Check the original specifier (marker_name), not the local alias.
                let pushed_marker = if marker_name != "$" {
                    ctx.push(&local_callee);
                    true
                } else {
                    false
                };

                // This is a $() call - extract the first argument as a segment
                if let Some(arg) = call.arguments.first() {
                    if let Some(arg_expr) = arg.as_expression() {
                        let (is_async, param_names) = get_function_info(arg_expr);
                        let body_span = arg_expr.span();

                        let body_text = source[body_span.start as usize..body_span.end as usize].to_string();

                        let ctx_kind = if is_event_handler_name(&marker_name) {
                            SegmentKind::EventHandler
                        } else {
                            SegmentKind::Function
                        };

                        let current_ctx: Vec<String> = ctx.as_slice().iter().map(|s| s.to_string()).collect();

                        // For identifier arguments, derive display/hash overrides from import
                        let (display_name_override, hash_seed_override) = if let Expression::Identifier(id) = arg_expr {
                            match get_import_qrl_info(&id.name) {
                                Some((dn, hs)) => (Some(dn), Some(hs)),
                                None => (None, None),
                            }
                        } else {
                            (None, None)
                        };

                        results.push(ExtractionResult {
                            marker_name: marker_name.clone(),
                            context_stack: current_ctx.clone(),
                            body_text,
                            start: body_span.start,
                            end: body_span.end,
                            call_start: call.span.start,
                            call_end: call.span.end,
                            is_async,
                            param_names,
                            in_jsx: false,
                            ctx_kind,
                            parent_segment: parent_segment.map(|s| s.to_string()),
                            has_jsx: source[body_span.start as usize..body_span.end as usize].contains("jsx")
                                || source[body_span.start as usize..body_span.end as usize].contains('<'),
                            display_name_override,
                            hash_seed_override,
                        });

                        // Recurse into the extracted body for nested extractions
                        let parent_name = format!("segment_{}", results.len() - 1);
                        extract_from_expr(source, arg_expr, results, ctx, marker_imports, Some(&parent_name));
                    }
                }

                // Also process remaining arguments (e.g., valiForm$(schema) as 2nd arg of formAction$)
                for arg in call.arguments.iter().skip(1) {
                    if let Some(arg_expr) = arg.as_expression() {
                        extract_from_expr(source, arg_expr, results, ctx, marker_imports, parent_segment);
                    }
                }

                if pushed_marker {
                    ctx.pop();
                }
            } else {
                // Not a marker call - push context for plain identifier callees only
                // (SWC pushes ident.sym for Callee::Expr(Ident), but NOT for member exprs)
                let pushed = if let Expression::Identifier(id) = &call.callee {
                    ctx.push(&id.name);
                    true
                } else {
                    false
                };

                // Recurse into arguments
                for arg in &call.arguments {
                    if let Some(expr) = arg.as_expression() {
                        extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
                    }
                }

                if pushed {
                    ctx.pop();
                }
            }
        }
        Expression::ArrowFunctionExpression(arrow) => {
            // Walk body for nested extractions
            extract_from_statements(source, &arrow.body.statements, results, ctx, marker_imports, parent_segment);
        }
        Expression::FunctionExpression(fn_expr) => {
            if let Some(ref body) = fn_expr.body {
                extract_from_statements(source, &body.statements, results, ctx, marker_imports, parent_segment);
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            extract_from_expr(source, &paren.expression, results, ctx, marker_imports, parent_segment);
        }
        Expression::ConditionalExpression(cond) => {
            extract_from_expr(source, &cond.consequent, results, ctx, marker_imports, parent_segment);
            extract_from_expr(source, &cond.alternate, results, ctx, marker_imports, parent_segment);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
            }
        }
        Expression::JSXElement(jsx) => {
            extract_from_jsx_element(source, jsx, results, ctx, marker_imports, parent_segment);
        }
        Expression::JSXFragment(frag) => {
            // SWC only pushes Fragment context when JSX is transpiled
            // (because <> becomes jsx(Fragment, ...) call which pushes callee name)
            let push_fragment = TRANSPILE_JSX.get();
            if push_fragment {
                ctx.push("Fragment");
            }
            for child in &frag.children {
                extract_from_jsx_child(source, child, results, ctx, marker_imports, parent_segment);
            }
            if push_fragment {
                ctx.pop();
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        extract_from_expr(source, &p.value, results, ctx, marker_imports, parent_segment);
                    }
                    ObjectPropertyKind::SpreadProperty(s) => {
                        extract_from_expr(source, &s.argument, results, ctx, marker_imports, parent_segment);
                    }
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(s) => {
                        extract_from_expr(source, &s.argument, results, ctx, marker_imports, parent_segment);
                    }
                    _ => {
                        if let Some(expr) = elem.as_expression() {
                            extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
                        }
                    }
                }
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for expr in &tpl.expressions {
                extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
            }
        }
        Expression::LogicalExpression(logical) => {
            extract_from_expr(source, &logical.left, results, ctx, marker_imports, parent_segment);
            extract_from_expr(source, &logical.right, results, ctx, marker_imports, parent_segment);
        }
        Expression::AssignmentExpression(assign) => {
            extract_from_expr(source, &assign.right, results, ctx, marker_imports, parent_segment);
        }
        Expression::BinaryExpression(bin) => {
            extract_from_expr(source, &bin.left, results, ctx, marker_imports, parent_segment);
            extract_from_expr(source, &bin.right, results, ctx, marker_imports, parent_segment);
        }
        Expression::UnaryExpression(unary) => {
            extract_from_expr(source, &unary.argument, results, ctx, marker_imports, parent_segment);
        }
        Expression::AwaitExpression(a) => {
            extract_from_expr(source, &a.argument, results, ctx, marker_imports, parent_segment);
        }
        _ => {}
    }
}

fn extract_from_jsx_element<'a>(
    source: &str,
    jsx: &JSXElement<'a>,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    // Push element tag name onto context stack
    let tag_name = get_jsx_element_name(&jsx.opening_element.name);
    ctx.push(&tag_name);

    // Detect whether this is a native HTML element (lowercase first char)
    let is_native = tag_name.chars().next().is_some_and(|c| c.is_ascii_lowercase());

    // Collect passive event names from passive:eventName attributes
    let passive_events: std::collections::HashSet<String> = jsx.opening_element.attributes.iter()
        .filter_map(|attr| {
            if let JSXAttributeItem::Attribute(a) = attr {
                let name = get_jsx_attr_name(&a.name);
                if let Some(event) = name.strip_prefix("passive:") {
                    return Some(event.to_string());
                }
            }
            None
        })
        .collect();

    // Walk attributes for event handlers
    for attr in &jsx.opening_element.attributes {
        match attr {
            JSXAttributeItem::Attribute(a) => {
                if let Some(ref value) = a.value {
                    match value {
                        JSXAttributeValue::ExpressionContainer(container) => {
                            if let Some(expr) = container.expression.as_expression() {
                                let attr_name = get_jsx_attr_name(&a.name);

                                // Transform event name for context:
                                // On native elements: onClick$ → q-e:click
                                // On components: keep original name
                                let context_name = if is_native {
                                    jsx_event_to_context_name(&attr_name, &passive_events).unwrap_or(attr_name.clone())
                                } else {
                                    attr_name.clone()
                                };

                                // If attribute name ends with $, extract value as a segment
                                // BUT only if the value is not already a $() call
                                if attr_name.ends_with('$') && !is_dollar_call(expr, marker_imports) {
                                    ctx.push(&context_name);
                                    let (is_async, param_names) = get_function_info(expr);
                                    let body_span = expr.span();
                                    let body_text = source[body_span.start as usize..body_span.end as usize].to_string();

                                    let ctx_kind = if is_event_handler_name(&attr_name) {
                                        SegmentKind::EventHandler
                                    } else {
                                        SegmentKind::Function
                                    };

                                    results.push(ExtractionResult {
                                        marker_name: attr_name.clone(),
                                        context_stack: ctx.as_slice().iter().map(|s| s.to_string()).collect(),
                                        body_text,
                                        start: body_span.start,
                                        end: body_span.end,
                                        call_start: body_span.start,
                                        call_end: body_span.end,
                                        is_async,
                                        param_names,
                                        in_jsx: true,
                                        ctx_kind,
                                        parent_segment: parent_segment.map(|s| s.to_string()),
                                        has_jsx: false,
                                        display_name_override: None,
                                        hash_seed_override: None,
                                    });

                                    // Recurse into the expression for nested segments
                                    extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
                                    ctx.pop();
                                } else {
                                    // Push attribute name as context
                                    ctx.push(&context_name);
                                    extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
                                    ctx.pop();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            JSXAttributeItem::SpreadAttribute(spread) => {
                extract_from_expr(source, &spread.argument, results, ctx, marker_imports, parent_segment);
            }
        }
    }

    // Walk children
    for child in &jsx.children {
        extract_from_jsx_child(source, child, results, ctx, marker_imports, parent_segment);
    }

    ctx.pop();
}

fn extract_from_jsx_child<'a>(
    source: &str,
    child: &JSXChild<'a>,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    match child {
        JSXChild::Element(el) => {
            extract_from_jsx_element(source, el, results, ctx, marker_imports, parent_segment);
        }
        JSXChild::ExpressionContainer(container) => {
            if let Some(expr) = container.expression.as_expression() {
                extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
            }
        }
        JSXChild::Fragment(frag) => {
            for c in &frag.children {
                extract_from_jsx_child(source, c, results, ctx, marker_imports, parent_segment);
            }
        }
        JSXChild::Spread(spread) => {
            extract_from_expr(source, &spread.expression, results, ctx, marker_imports, parent_segment);
        }
        _ => {}
    }
}

fn get_jsx_element_name(name: &JSXElementName) -> String {
    match name {
        JSXElementName::Identifier(id) => id.name.to_string(),
        JSXElementName::IdentifierReference(id) => id.name.to_string(),
        JSXElementName::NamespacedName(ns) => format!("{}:{}", ns.namespace.name, ns.name.name),
        JSXElementName::MemberExpression(member) => {
            format!("{}.{}", get_jsx_member_object(&member.object), member.property.name)
        }
        JSXElementName::ThisExpression(_) => "this".to_string(),
    }
}

/// Derive display_name and hash_seed from an identifier's import source.
/// Returns (display_name_override, hash_seed) for import-based QRL arguments.
fn get_import_qrl_info(ident_name: &str) -> Option<(String, String)> {
    CURRENT_IMPORTS.with(|imports| {
        let imports = imports.borrow();
        imports.iter().find(|imp| imp.local_name == ident_name).map(|imp| {
            // Display name from import source + specifier
            let source_stem = imp.source
                .rsplit('/')
                .next()
                .unwrap_or(&imp.source)
                .trim_start_matches('.')
                .replace(".css", "_css")
                .replace(".js", "")
                .replace(".ts", "")
                .replace(".mjs", "");
            let display_name = if imp.specifier == "default" {
                source_stem
            } else {
                format!("{}_{}", source_stem, imp.specifier)
            };
            // Hash seed: "resolved_source#specifier" (matches SWC's ImportQrlName.hash_seed)
            // Resolve relative paths by stripping leading "./"
            let resolved_source = imp.source.replace('\\', "/");
            let resolved_source = if resolved_source.starts_with("./") {
                &resolved_source[2..]
            } else {
                &resolved_source
            };
            let hash_seed = format!("{}#{}", resolved_source, imp.specifier);
            (display_name, hash_seed)
        })
    })
}

/// Get the local callee identifier name (what the user wrote in source).
fn get_local_callee_name(callee: &Expression) -> Option<String> {
    match callee {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn get_jsx_member_object(obj: &JSXMemberExpressionObject) -> String {
    match obj {
        JSXMemberExpressionObject::IdentifierReference(id) => id.name.to_string(),
        JSXMemberExpressionObject::MemberExpression(member) => {
            format!("{}.{}", get_jsx_member_object(&member.object), member.property.name)
        }
        JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
    }
}

fn get_jsx_attr_name(name: &JSXAttributeName) -> String {
    match name {
        JSXAttributeName::Identifier(id) => id.name.to_string(),
        JSXAttributeName::NamespacedName(ns) => format!("{}:{}", ns.namespace.name, ns.name.name),
    }
}

/// Convert JSX event name to HTML attribute name for context stack.
/// onClick$ → q-e:click, onInput$ → q-e:input, window:onClick$ → q-w:click
/// Follows SWC's jsx_event_to_html_attribute logic.
fn jsx_event_to_context_name(
    jsx_name: &str,
    passive_events: &std::collections::HashSet<String>,
) -> Option<String> {
    if !jsx_name.ends_with('$') {
        return None;
    }

    // Extract event name for passive check
    let event_name_for_passive = jsx_event_to_event_name(jsx_name);
    let is_passive = event_name_for_passive
        .as_ref()
        .is_some_and(|e| passive_events.contains(e));

    let (prefix, idx) = if jsx_name.starts_with("window:on") {
        (if is_passive { "q-wp:" } else { "q-w:" }, 9usize)
    } else if jsx_name.starts_with("document:on") {
        (if is_passive { "q-dp:" } else { "q-d:" }, 11)
    } else if jsx_name.starts_with("on") {
        (if is_passive { "q-ep:" } else { "q-e:" }, 2)
    } else {
        return None;
    };

    // Strip the $ suffix and get the event name portion
    let event_part = &jsx_name[idx..jsx_name.len() - 1];

    // SWC normalize_jsx_event_name logic:
    // If event part starts with '-', it's a case-sensitive marker.
    // Strip the '-' and apply create_event_name (camelCase to kebab).
    // Otherwise, lowercase everything first.
    let normalized = if let Some(stripped) = event_part.strip_prefix('-') {
        // Case-sensitive: apply create_event_name on original case
        let mut result = String::new();
        for c in stripped.chars() {
            if c.is_ascii_uppercase() || c == '-' {
                result.push('-');
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c);
            }
        }
        result
    } else {
        // Standard: lowercase everything
        event_part.to_lowercase()
    };

    Some(format!("{}{}", prefix, normalized))
}

/// Extract the raw event name from a JSX event prop name.
/// onClick$ → click, window:onScroll$ → scroll
fn jsx_event_to_event_name(jsx_name: &str) -> Option<String> {
    if !jsx_name.ends_with('$') {
        return None;
    }

    let idx = if jsx_name.starts_with("window:on") {
        9
    } else if jsx_name.starts_with("document:on") {
        11
    } else if jsx_name.starts_with("on") {
        2
    } else {
        return None;
    };

    let event_part = &jsx_name[idx..jsx_name.len() - 1];
    Some(event_part.to_lowercase())
}

/// Check if an expression is a bare $() call or other marker call.
fn is_dollar_call(
    expr: &Expression,
    marker_imports: &std::collections::HashMap<String, String>,
) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            get_call_marker_name(call, marker_imports).is_some()
        }
        Expression::ParenthesizedExpression(paren) => {
            is_dollar_call(&paren.expression, marker_imports)
        }
        _ => false,
    }
}

fn extract_from_expression_or_decl(
    source: &str,
    export_default: &ExportDefaultDeclarationKind,
    results: &mut Vec<ExtractionResult>,
    ctx: &mut ContextStack,
    marker_imports: &std::collections::HashMap<String, String>,
    parent_segment: Option<&str>,
) {
    match export_default {
        ExportDefaultDeclarationKind::FunctionDeclaration(fn_decl) => {
            let name = fn_decl.id.as_ref().map(|id| id.name.to_string()).unwrap_or_else(|| "default".to_string());
            ctx.push(&name);
            if let Some(ref body) = fn_decl.body {
                extract_from_statements(source, &body.statements, results, ctx, marker_imports, parent_segment);
            }
            ctx.pop();
        }
        ExportDefaultDeclarationKind::ClassDeclaration(_) => {}
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {}
        _ => {
            if let Some(expr) = export_default.as_expression() {
                extract_from_expr(source, expr, results, ctx, marker_imports, parent_segment);
            }
        }
    }
}

/// Get the marker function name if this call is a $-suffixed call.
fn get_call_marker_name(
    call: &CallExpression,
    marker_imports: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match &call.callee {
        Expression::Identifier(id) => {
            let name = id.name.as_str();
            // Check if it's a direct marker or an imported marker
            if is_marker_function(name) {
                return Some(name.to_string());
            }
            if let Some(original) = marker_imports.get(name) {
                return Some(original.clone());
            }
            None
        }
        Expression::StaticMemberExpression(member) => {
            let prop = member.property.name.as_str();
            if is_marker_function(prop) {
                Some(prop.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_event_handler_name(name: &str) -> bool {
    name.starts_with("on") && name.ends_with('$') && name.len() > 3
}

fn get_binding_name(pattern: &BindingPattern) -> Option<String> {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn get_function_info(expr: &Expression) -> (bool, Vec<String>) {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => {
            let params: Vec<String> = arrow
                .params
                .items
                .iter()
                .filter_map(|p| get_binding_name(&p.pattern))
                .collect();
            (arrow.r#async, params)
        }
        Expression::FunctionExpression(fn_expr) => {
            let params: Vec<String> = fn_expr
                .params
                .items
                .iter()
                .filter_map(|p| get_binding_name(&p.pattern))
                .collect();
            (fn_expr.r#async, params)
        }
        Expression::ParenthesizedExpression(paren) => {
            get_function_info(&paren.expression)
        }
        _ => (false, vec![]),
    }
}

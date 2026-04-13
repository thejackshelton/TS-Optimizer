// Signal analysis for JSX prop expressions.
//
// Detects signal/store patterns and generates _wrapProp or _fnSignal representations.
// Operates on source text with OXC AST analysis.

use std::collections::{BTreeSet, HashMap};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

// ---------------------------------------------------------------------------
// Well-known globals that are NOT reactive
// ---------------------------------------------------------------------------

fn is_global_name(name: &str) -> bool {
    matches!(
        name,
        "window"
            | "document"
            | "globalThis"
            | "navigator"
            | "location"
            | "history"
            | "screen"
            | "localStorage"
            | "sessionStorage"
            | "console"
            | "Math"
            | "JSON"
            | "Date"
            | "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "undefined"
            | "NaN"
            | "Infinity"
            | "isServer"
            | "isBrowser"
    )
}

// ---------------------------------------------------------------------------
// Signal expression analysis result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SignalExprResult {
    /// Not a reactive expression, leave as-is
    None,
    /// Simple signal.value → _wrapProp(signal) or _wrapProp(signal, "prop")
    WrapProp { code: String },
    /// Complex expression → _fnSignal(_hfN, [deps], _hfN_str)
    FnSignal {
        deps: Vec<String>,
        hoisted_fn: String,
        hoisted_str: String,
    },
}

// ---------------------------------------------------------------------------
// Hoisted function manager
// ---------------------------------------------------------------------------

/// Manages hoisted signal functions (_hf0, _hf1, etc.) for a module/segment.
#[derive(Debug, Default)]
pub struct SignalHoister {
    pub counter: u32,
    pub hoisted_functions: Vec<HoistedFn>,
    dedup_map: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct HoistedFn {
    pub name: String,
    pub fn_text: String,
    pub str_text: String,
}

impl SignalHoister {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a hoisted function, returns the _hfN name.
    /// Deduplicates: if an identical function body already exists, reuses its name.
    pub fn hoist(&mut self, fn_text: &str, str_text: &str) -> String {
        if let Some(existing) = self.dedup_map.get(fn_text) {
            return existing.clone();
        }

        let name = format!("_hf{}", self.counter);
        self.counter += 1;
        self.hoisted_functions.push(HoistedFn {
            name: name.clone(),
            fn_text: fn_text.to_string(),
            str_text: str_text.to_string(),
        });
        self.dedup_map.insert(fn_text.to_string(), name.clone());
        name
    }

    /// Get all hoisted declarations as source text lines.
    pub fn get_declarations(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for h in &self.hoisted_functions {
            lines.push(format!("const {} = {};", h.name, h.fn_text));
            lines.push(format!("const {}_str = {};", h.name, h.str_text));
        }
        lines
    }

    pub fn is_empty(&self) -> bool {
        self.hoisted_functions.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Expression analysis
// ---------------------------------------------------------------------------

/// Check if a MemberExpression is a `.value` access (signal pattern).
fn is_signal_value_access(expr: &Expression) -> bool {
    if let Expression::StaticMemberExpression(member) = expr {
        member.property.name.as_str() == "value"
    } else {
        false
    }
}

/// Get the root identifier name from a member chain.
/// `store.address.city.name` → Some("store")
fn get_member_chain_root(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::StaticMemberExpression(member) => get_member_chain_root(&member.object),
        Expression::ComputedMemberExpression(member) => get_member_chain_root(&member.object),
        _ => None,
    }
}

/// Get the depth of a member expression chain.
fn get_member_chain_depth(expr: &Expression) -> u32 {
    match expr {
        Expression::StaticMemberExpression(member) => 1 + get_member_chain_depth(&member.object),
        Expression::ComputedMemberExpression(member) => {
            1 + get_member_chain_depth(&member.object)
        }
        _ => 0,
    }
}

/// Check if expression is a deep store access (depth >= 2) on a non-imported identifier.
/// The local_names set is used as a hint: if provided and non-empty, the identifier
/// must be in it. If empty, any non-imported/non-global name is treated as local.
fn is_deep_store_access(
    expr: &Expression,
    imported_names: &BTreeSet<String>,
    _local_names: &BTreeSet<String>,
) -> bool {
    let depth = get_member_chain_depth(expr);
    if depth < 2 {
        return false;
    }
    let root = get_member_chain_root(expr);
    if let Some(ref name) = root {
        if imported_names.contains(name) || is_global_name(name) {
            return false;
        }
        // If local_names is provided (non-empty), check membership.
        // But also allow names not in either set (e.g., nested function params like `row`).
        // The SWC optimizer treats any non-imported, non-global identifier as potentially reactive.
        true
    } else {
        false
    }
}

/// Check if expression is a single-level store field access (e.g., props.field).
fn is_store_field_access(
    expr: &Expression,
    imported_names: &BTreeSet<String>,
    _local_names: &BTreeSet<String>,
) -> bool {
    if let Expression::StaticMemberExpression(member) = expr {
        if let Expression::Identifier(obj_id) = &member.object {
            let name = obj_id.name.as_str();
            if imported_names.contains(name) || is_global_name(name) {
                return false;
            }
            // Exclude .value access (that's signal, not store field)
            if member.property.name.as_str() == "value" {
                return false;
            }
            return true;
        }
    }
    false
}

/// Check if expression contains a function call that is NOT a method call
/// and NOT to an imported name (an "unknown call").
fn contains_unknown_call(expr: &Expression, imported_names: &BTreeSet<String>) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            // Method calls (obj.method()) are allowed
            match &call.callee {
                Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
                    // Method call — check arguments but don't flag this
                }
                Expression::ChainExpression(_) => {}
                Expression::Identifier(id) => {
                    if !imported_names.contains(id.name.as_str()) {
                        return true;
                    }
                }
                _ => return true,
            }
            // Check arguments
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    if contains_unknown_call(arg_expr, imported_names) {
                        return true;
                    }
                }
            }
            false
        }
        Expression::TaggedTemplateExpression(_) => true,
        Expression::BinaryExpression(bin) => {
            contains_unknown_call(&bin.left, imported_names)
                || contains_unknown_call(&bin.right, imported_names)
        }
        Expression::LogicalExpression(log) => {
            contains_unknown_call(&log.left, imported_names)
                || contains_unknown_call(&log.right, imported_names)
        }
        Expression::ConditionalExpression(cond) => {
            contains_unknown_call(&cond.test, imported_names)
                || contains_unknown_call(&cond.consequent, imported_names)
                || contains_unknown_call(&cond.alternate, imported_names)
        }
        Expression::UnaryExpression(un) => contains_unknown_call(&un.argument, imported_names),
        Expression::TemplateLiteral(tpl) => tpl
            .expressions
            .iter()
            .any(|e| contains_unknown_call(e, imported_names)),
        Expression::ParenthesizedExpression(p) => {
            contains_unknown_call(&p.expression, imported_names)
        }
        Expression::ArrayExpression(arr) => arr.elements.iter().any(|el| match el {
            ArrayExpressionElement::SpreadElement(s) => {
                contains_unknown_call(&s.argument, imported_names)
            }
            _ => {
                if let Some(e) = el.as_expression() {
                    contains_unknown_call(e, imported_names)
                } else {
                    false
                }
            }
        }),
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|prop| match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                contains_unknown_call(&p.value, imported_names)
            }
            ObjectPropertyKind::SpreadProperty(s) => {
                contains_unknown_call(&s.argument, imported_names)
            }
        }),
        Expression::StaticMemberExpression(m) => contains_unknown_call(&m.object, imported_names),
        Expression::ComputedMemberExpression(m) => {
            contains_unknown_call(&m.object, imported_names)
                || contains_unknown_call(&m.expression, imported_names)
        }
        _ => false,
    }
}

/// Check if expression contains a direct reference to an imported name.
fn contains_imported_reference(expr: &Expression, imported_names: &BTreeSet<String>) -> bool {
    match expr {
        Expression::Identifier(id) => imported_names.contains(id.name.as_str()),
        Expression::BinaryExpression(bin) => {
            contains_imported_reference(&bin.left, imported_names)
                || contains_imported_reference(&bin.right, imported_names)
        }
        Expression::LogicalExpression(log) => {
            contains_imported_reference(&log.left, imported_names)
                || contains_imported_reference(&log.right, imported_names)
        }
        Expression::ConditionalExpression(cond) => {
            contains_imported_reference(&cond.test, imported_names)
                || contains_imported_reference(&cond.consequent, imported_names)
                || contains_imported_reference(&cond.alternate, imported_names)
        }
        Expression::UnaryExpression(un) => {
            contains_imported_reference(&un.argument, imported_names)
        }
        Expression::TemplateLiteral(tpl) => tpl
            .expressions
            .iter()
            .any(|e| contains_imported_reference(e, imported_names)),
        Expression::ParenthesizedExpression(p) => {
            contains_imported_reference(&p.expression, imported_names)
        }
        Expression::ArrayExpression(arr) => arr.elements.iter().any(|el| {
            if let Some(e) = el.as_expression() {
                contains_imported_reference(e, imported_names)
            } else {
                false
            }
        }),
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|prop| match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                contains_imported_reference(&p.value, imported_names)
            }
            ObjectPropertyKind::SpreadProperty(s) => {
                contains_imported_reference(&s.argument, imported_names)
            }
        }),
        // Don't recurse into member access property names
        Expression::StaticMemberExpression(m) => {
            contains_imported_reference(&m.object, imported_names)
        }
        Expression::ComputedMemberExpression(m) => {
            contains_imported_reference(&m.object, imported_names)
                || contains_imported_reference(&m.expression, imported_names)
        }
        _ => false,
    }
}

/// Collect reactive roots from an expression.
/// Returns unique root names in encounter order.
fn collect_reactive_roots(
    expr: &Expression,
    imported_names: &BTreeSet<String>,
    local_names: &BTreeSet<String>,
) -> Vec<String> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();

    fn walk(
        expr: &Expression,
        imported_names: &BTreeSet<String>,
        local_names: &BTreeSet<String>,
        roots: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) {
        // signal.value access -> root is the object name
        if is_signal_value_access(expr) {
            if let Expression::StaticMemberExpression(member) = expr {
                if let Some(root) = get_member_chain_root(&member.object) {
                    if seen.insert(root.clone()) {
                        roots.push(root);
                    }
                }
            }
            return;
        }

        // Deep store access -> root is the chain root
        if is_deep_store_access(expr, imported_names, local_names) {
            if let Some(root) = get_member_chain_root(expr) {
                if seen.insert(root.clone()) {
                    roots.push(root);
                }
            }
            return;
        }

        // Single-level store field access
        if is_store_field_access(expr, imported_names, local_names) {
            if let Expression::StaticMemberExpression(member) = expr {
                if let Some(root) = get_member_chain_root(&member.object) {
                    if seen.insert(root.clone()) {
                        roots.push(root);
                    }
                }
            }
            return;
        }

        // Recurse into sub-expressions
        match expr {
            Expression::BinaryExpression(bin) => {
                walk(&bin.left, imported_names, local_names, roots, seen);
                walk(&bin.right, imported_names, local_names, roots, seen);
            }
            Expression::LogicalExpression(log) => {
                walk(&log.left, imported_names, local_names, roots, seen);
                walk(&log.right, imported_names, local_names, roots, seen);
            }
            Expression::ConditionalExpression(cond) => {
                walk(&cond.test, imported_names, local_names, roots, seen);
                walk(&cond.consequent, imported_names, local_names, roots, seen);
                walk(&cond.alternate, imported_names, local_names, roots, seen);
            }
            Expression::UnaryExpression(un) => {
                walk(&un.argument, imported_names, local_names, roots, seen);
            }
            Expression::TemplateLiteral(tpl) => {
                for e in &tpl.expressions {
                    walk(e, imported_names, local_names, roots, seen);
                }
            }
            Expression::ParenthesizedExpression(p) => {
                walk(&p.expression, imported_names, local_names, roots, seen);
            }
            Expression::ArrayExpression(arr) => {
                for el in &arr.elements {
                    if let Some(e) = el.as_expression() {
                        walk(e, imported_names, local_names, roots, seen);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            walk(&p.value, imported_names, local_names, roots, seen);
                        }
                        ObjectPropertyKind::SpreadProperty(s) => {
                            walk(&s.argument, imported_names, local_names, roots, seen);
                        }
                    }
                }
            }
            Expression::StaticMemberExpression(m) => {
                walk(&m.object, imported_names, local_names, roots, seen);
            }
            Expression::ComputedMemberExpression(m) => {
                walk(&m.object, imported_names, local_names, roots, seen);
                walk(&m.expression, imported_names, local_names, roots, seen);
            }
            _ => {}
        }
    }

    walk(expr, imported_names, local_names, &mut roots, &mut seen);
    roots
}

/// Collect all dependency identifiers (reactive roots + bare locals).
/// Returns sorted unique names.
fn collect_all_deps(expr: &Expression, imported_names: &BTreeSet<String>) -> Vec<String> {
    let mut deps = Vec::new();
    let mut seen = BTreeSet::new();

    fn walk(
        expr: &Expression,
        imported_names: &BTreeSet<String>,
        deps: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) {
        // signal.value access -> reactive root
        if is_signal_value_access(expr) {
            if let Expression::StaticMemberExpression(member) = expr {
                if let Some(root) = get_member_chain_root(&member.object) {
                    if seen.insert(root.clone()) {
                        deps.push(root);
                    }
                }
            }
            return;
        }

        // Deep store access -> reactive root
        if get_member_chain_depth(expr) >= 2 {
            if let Some(root) = get_member_chain_root(expr) {
                if !imported_names.contains(&root) && !is_global_name(&root) {
                    if seen.insert(root.clone()) {
                        deps.push(root);
                    }
                    return;
                }
            }
        }

        // Single-level store field access
        if let Expression::StaticMemberExpression(member) = expr {
            if let Expression::Identifier(obj_id) = &member.object {
                let name = obj_id.name.as_str();
                if !imported_names.contains(name)
                    && !is_global_name(name)
                    && member.property.name.as_str() != "value"
                {
                    if seen.insert(name.to_string()) {
                        deps.push(name.to_string());
                    }
                    return;
                }
            }
        }

        // Bare local identifier
        if let Expression::Identifier(id) = expr {
            let name = id.name.as_str();
            if !imported_names.contains(name) && !is_global_name(name) {
                if seen.insert(name.to_string()) {
                    deps.push(name.to_string());
                }
            }
            return;
        }

        // Recurse
        match expr {
            Expression::BinaryExpression(bin) => {
                walk(&bin.left, imported_names, deps, seen);
                walk(&bin.right, imported_names, deps, seen);
            }
            Expression::LogicalExpression(log) => {
                walk(&log.left, imported_names, deps, seen);
                walk(&log.right, imported_names, deps, seen);
            }
            Expression::ConditionalExpression(cond) => {
                walk(&cond.test, imported_names, deps, seen);
                walk(&cond.consequent, imported_names, deps, seen);
                walk(&cond.alternate, imported_names, deps, seen);
            }
            Expression::UnaryExpression(un) => {
                walk(&un.argument, imported_names, deps, seen);
            }
            Expression::TemplateLiteral(tpl) => {
                for e in &tpl.expressions {
                    walk(e, imported_names, deps, seen);
                }
            }
            Expression::ParenthesizedExpression(p) => {
                walk(&p.expression, imported_names, deps, seen);
            }
            Expression::ArrayExpression(arr) => {
                for el in &arr.elements {
                    if let Some(e) = el.as_expression() {
                        walk(e, imported_names, deps, seen);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            // Don't walk into property keys
                            walk(&p.value, imported_names, deps, seen);
                        }
                        ObjectPropertyKind::SpreadProperty(s) => {
                            walk(&s.argument, imported_names, deps, seen);
                        }
                    }
                }
            }
            // Don't count property names as deps
            Expression::StaticMemberExpression(m) => {
                walk(&m.object, imported_names, deps, seen);
            }
            Expression::ComputedMemberExpression(m) => {
                walk(&m.object, imported_names, deps, seen);
                walk(&m.expression, imported_names, deps, seen);
            }
            _ => {}
        }
    }

    walk(expr, imported_names, &mut deps, &mut seen);
    deps.sort();
    deps
}

// ---------------------------------------------------------------------------
// ChainExpression helpers (optional chaining)
// ---------------------------------------------------------------------------

/// Get the root identifier name from a ChainElement (unwrapping member chains).
fn get_chain_root(element: &ChainElement) -> Option<String> {
    match element {
        ChainElement::CallExpression(call) => {
            // The callee of a chain call is a ChainElement-compatible expression.
            // For `signal.formData?.get('username')`, callee is a member expression.
            get_member_chain_root(&call.callee)
        }
        ChainElement::StaticMemberExpression(member) => {
            get_member_chain_root(&member.object)
        }
        ChainElement::ComputedMemberExpression(member) => {
            get_member_chain_root(&member.object)
        }
        ChainElement::PrivateFieldExpression(pfe) => {
            get_member_chain_root(&pfe.object)
        }
        _ => None,
    }
}

/// Collect dependency roots from a ChainElement for _fnSignal.
fn collect_all_deps_from_chain(
    element: &ChainElement,
    imported_names: &BTreeSet<String>,
) -> Vec<String> {
    let mut deps = Vec::new();
    let mut seen = BTreeSet::new();

    // Extract root identifier
    let root = get_chain_root(element);
    if let Some(ref name) = root {
        if !imported_names.contains(name.as_str()) && !is_global_name(name) {
            if seen.insert(name.clone()) {
                deps.push(name.clone());
            }
        }
    }

    deps.sort();
    deps
}

// ---------------------------------------------------------------------------
// fnSignal generation
// ---------------------------------------------------------------------------

/// Generate hoisted function body by replacing root identifiers with pN params.
fn generate_fn_signal(
    expr_text: &str,
    expr: &Expression,
    deps: &[String],
) -> (String, String) {
    let dep_to_param: HashMap<&str, String> = deps
        .iter()
        .enumerate()
        .map(|(i, d)| (d.as_str(), format!("p{}", i)))
        .collect();

    let expr_start = expr.span().start as usize;

    // Collect positions of identifiers to replace
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    collect_identifier_replacements(expr, &dep_to_param, expr_start, &mut replacements);

    // Sort by position ascending
    replacements.sort_by_key(|r| r.0);

    // Build function body with replacements
    let mut fn_body = String::new();
    let mut pos = 0;
    for (start, end, replacement) in &replacements {
        fn_body.push_str(&expr_text[pos..*start]);
        fn_body.push_str(replacement);
        pos = *end;
    }
    fn_body.push_str(&expr_text[pos..]);

    // Params
    let params: String = deps
        .iter()
        .enumerate()
        .map(|(i, _)| format!("p{}", i))
        .collect::<Vec<_>>()
        .join(", ");

    // Check if expression is an object expression (needs parens)
    let needs_parens = matches!(expr, Expression::ObjectExpression(_));
    let hoisted_fn = if needs_parens {
        format!("({})=>({})", params, fn_body)
    } else {
        format!("({})=>{}", params, fn_body)
    };

    // Generate minified string representation
    let str_body = minify_expr_string(&fn_body);

    // Choose quote style based on content
    let hoisted_str = if str_body.contains('"') {
        format!("'{}'", str_body)
    } else {
        format!("\"{}\"", str_body)
    };

    (hoisted_fn, hoisted_str)
}

/// Collect all identifier positions that need to be replaced with pN params.
fn collect_identifier_replacements(
    expr: &Expression,
    dep_to_param: &HashMap<&str, String>,
    base_offset: usize,
    replacements: &mut Vec<(usize, usize, String)>,
) {
    // signal.value access -> replace root identifier
    if is_signal_value_access(expr) {
        if let Expression::StaticMemberExpression(member) = expr {
            collect_root_identifier_replacement(&member.object, dep_to_param, base_offset, replacements);
        }
        return;
    }

    // Deep store access (depth >= 2) -> replace root identifier
    if get_member_chain_depth(expr) >= 2 {
        if let Some(root) = get_member_chain_root(expr) {
            if dep_to_param.contains_key(root.as_str()) {
                collect_root_identifier_replacement(expr, dep_to_param, base_offset, replacements);
                return;
            }
        }
    }

    // Single-level store field access
    if let Expression::StaticMemberExpression(member) = expr {
        if let Expression::Identifier(obj_id) = &member.object {
            if dep_to_param.contains_key(obj_id.name.as_str())
                && member.property.name.as_str() != "value"
            {
                let start = obj_id.span.start as usize - base_offset;
                let end = obj_id.span.end as usize - base_offset;
                if let Some(param) = dep_to_param.get(obj_id.name.as_str()) {
                    replacements.push((start, end, param.clone()));
                }
                return;
            }
        }
    }

    // Bare identifier
    if let Expression::Identifier(id) = expr {
        if let Some(param) = dep_to_param.get(id.name.as_str()) {
            let start = id.span.start as usize - base_offset;
            let end = id.span.end as usize - base_offset;
            replacements.push((start, end, param.clone()));
        }
        return;
    }

    // Recurse
    match expr {
        Expression::BinaryExpression(bin) => {
            collect_identifier_replacements(&bin.left, dep_to_param, base_offset, replacements);
            collect_identifier_replacements(&bin.right, dep_to_param, base_offset, replacements);
        }
        Expression::LogicalExpression(log) => {
            collect_identifier_replacements(&log.left, dep_to_param, base_offset, replacements);
            collect_identifier_replacements(&log.right, dep_to_param, base_offset, replacements);
        }
        Expression::ConditionalExpression(cond) => {
            collect_identifier_replacements(&cond.test, dep_to_param, base_offset, replacements);
            collect_identifier_replacements(
                &cond.consequent,
                dep_to_param,
                base_offset,
                replacements,
            );
            collect_identifier_replacements(
                &cond.alternate,
                dep_to_param,
                base_offset,
                replacements,
            );
        }
        Expression::UnaryExpression(un) => {
            collect_identifier_replacements(&un.argument, dep_to_param, base_offset, replacements);
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_identifier_replacements(e, dep_to_param, base_offset, replacements);
            }
        }
        Expression::ParenthesizedExpression(p) => {
            collect_identifier_replacements(&p.expression, dep_to_param, base_offset, replacements);
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    collect_identifier_replacements(e, dep_to_param, base_offset, replacements);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        // Only recurse into value, not key
                        collect_identifier_replacements(
                            &p.value,
                            dep_to_param,
                            base_offset,
                            replacements,
                        );
                    }
                    ObjectPropertyKind::SpreadProperty(s) => {
                        collect_identifier_replacements(
                            &s.argument,
                            dep_to_param,
                            base_offset,
                            replacements,
                        );
                    }
                }
            }
        }
        Expression::StaticMemberExpression(m) => {
            collect_identifier_replacements(&m.object, dep_to_param, base_offset, replacements);
        }
        Expression::ComputedMemberExpression(m) => {
            collect_identifier_replacements(&m.object, dep_to_param, base_offset, replacements);
            collect_identifier_replacements(
                &m.expression,
                dep_to_param,
                base_offset,
                replacements,
            );
        }
        Expression::CallExpression(call) => {
            if let Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) =
                &call.callee
            {
                collect_identifier_replacements(
                    &call.callee,
                    dep_to_param,
                    base_offset,
                    replacements,
                );
            }
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_identifier_replacements(e, dep_to_param, base_offset, replacements);
                }
            }
        }
        Expression::ChainExpression(chain) => {
            // Unwrap the chain and replace root identifiers inside it
            collect_chain_identifier_replacements(&chain.expression, dep_to_param, base_offset, replacements);
        }
        _ => {}
    }
}

/// Find the root identifier in a member chain and replace it.
fn collect_root_identifier_replacement(
    expr: &Expression,
    dep_to_param: &HashMap<&str, String>,
    base_offset: usize,
    replacements: &mut Vec<(usize, usize, String)>,
) {
    match expr {
        Expression::Identifier(id) => {
            if let Some(param) = dep_to_param.get(id.name.as_str()) {
                let start = id.span.start as usize - base_offset;
                let end = id.span.end as usize - base_offset;
                replacements.push((start, end, param.clone()));
            }
        }
        Expression::StaticMemberExpression(m) => {
            collect_root_identifier_replacement(&m.object, dep_to_param, base_offset, replacements);
        }
        Expression::ComputedMemberExpression(m) => {
            collect_root_identifier_replacement(&m.object, dep_to_param, base_offset, replacements);
        }
        _ => {}
    }
}

/// Collect identifier replacements inside a ChainElement (optional chaining).
fn collect_chain_identifier_replacements(
    element: &ChainElement,
    dep_to_param: &HashMap<&str, String>,
    base_offset: usize,
    replacements: &mut Vec<(usize, usize, String)>,
) {
    match element {
        ChainElement::CallExpression(call) => {
            // Recurse into callee (which is an Expression)
            collect_identifier_replacements(&call.callee, dep_to_param, base_offset, replacements);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_identifier_replacements(e, dep_to_param, base_offset, replacements);
                }
            }
        }
        ChainElement::StaticMemberExpression(member) => {
            collect_identifier_replacements(&member.object, dep_to_param, base_offset, replacements);
        }
        ChainElement::ComputedMemberExpression(member) => {
            collect_identifier_replacements(&member.object, dep_to_param, base_offset, replacements);
            collect_identifier_replacements(&member.expression, dep_to_param, base_offset, replacements);
        }
        ChainElement::PrivateFieldExpression(pfe) => {
            collect_identifier_replacements(&pfe.object, dep_to_param, base_offset, replacements);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Minification for string representation
// ---------------------------------------------------------------------------

/// Minify expression text to match SWC's minimal re-serialization.
/// Removes whitespace, normalizes quotes to double, strips trailing commas.
fn minify_expr_string(text: &str) -> String {
    let tokens = tokenize_for_minify(text);

    // Join tokens, inserting space only where needed
    let mut result = String::new();
    for (i, tok) in tokens.iter().enumerate() {
        if i > 0 {
            let prev_last = result.chars().last().unwrap_or('\0');
            let cur_first = tok.chars().next().unwrap_or('\0');
            if is_word_char(prev_last) && is_word_char(cur_first) {
                result.push(' ');
            }
        }
        result.push_str(tok);
    }

    // Normalize single quotes to double quotes
    result = normalize_string_quotes(&result);
    // Strip trailing commas
    result = strip_trailing_commas(&result);

    result
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Tokenize text for minification — skip whitespace, preserve strings/templates.
fn tokenize_for_minify(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // Skip whitespace
        if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' {
            i += 1;
            continue;
        }

        // String literal
        if ch == '"' || ch == '\'' {
            let mut tok = String::new();
            tok.push(ch);
            i += 1;
            while i < chars.len() && chars[i] != ch {
                if chars[i] == '\\' {
                    tok.push(chars[i]);
                    i += 1;
                    if i < chars.len() {
                        tok.push(chars[i]);
                        i += 1;
                    }
                } else {
                    tok.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() {
                tok.push(chars[i]);
                i += 1;
            }
            tokens.push(tok);
            continue;
        }

        // Template literal
        if ch == '`' {
            let mut tok = String::new();
            tok.push(ch);
            i += 1;
            while i < chars.len() && chars[i] != '`' {
                tok.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                tok.push(chars[i]);
                i += 1;
            }
            tokens.push(tok);
            continue;
        }

        // Run of non-whitespace, non-string chars
        let mut tok = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                break;
            }
            if c == '"' || c == '\'' || c == '`' {
                break;
            }
            tok.push(c);
            i += 1;
        }
        if !tok.is_empty() {
            tokens.push(tok);
        }
    }

    tokens
}

/// Normalize single-quoted strings to double-quoted.
fn normalize_string_quotes(text: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\'' {
            // Convert single-quoted to double-quoted
            result.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                if chars[i] == '\\' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        // Escaped single quote -> just the quote
                        result.push('\'');
                        i += 2;
                    } else {
                        result.push(chars[i]);
                        i += 1;
                        if i < chars.len() {
                            result.push(chars[i]);
                            i += 1;
                        }
                    }
                } else if chars[i] == '"' {
                    result.push('\\');
                    result.push('"');
                    i += 1;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            result.push('"');
            if i < chars.len() {
                i += 1;
            } // skip closing '
        } else if chars[i] == '"' {
            // Already double-quoted, pass through
            result.push(chars[i]);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    result.push(chars[i]);
                    i += 1;
                    if i < chars.len() {
                        result.push(chars[i]);
                        i += 1;
                    }
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Strip trailing commas before closing braces/brackets/parens.
fn strip_trailing_commas(text: &str) -> String {
    let mut result = text.to_string();
    // Iteratively replace `,}`, `,]`, `,)` patterns
    loop {
        let new = result
            .replace(",}", "}")
            .replace(",]", "]")
            .replace(",)", ")");
        if new == result {
            break;
        }
        result = new;
    }
    result
}

// ---------------------------------------------------------------------------
// Main analysis entry point (source-text based)
// ---------------------------------------------------------------------------

/// Analyze a JSX prop expression (given as source text) and determine
/// if it needs signal wrapping.
///
/// Returns SignalExprResult with the wrapping info.
pub fn analyze_signal_expr_text(
    expr_text: &str,
    imported_names: &BTreeSet<String>,
    local_names: &BTreeSet<String>,
) -> SignalExprResult {
    // Parse the expression
    let wrapper = format!("const __x = {};", expr_text);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapper, source_type).parse();

    if parse_result.panicked || !parse_result.errors.is_empty() {
        return SignalExprResult::None;
    }

    // Extract the expression from `const __x = EXPR;`
    let expr = if let Some(Statement::VariableDeclaration(decl)) =
        parse_result.program.body.first()
    {
        if let Some(declarator) = decl.declarations.first() {
            declarator.init.as_ref()
        } else {
            None
        }
    } else {
        None
    };

    let expr = match expr {
        Some(e) => e,
        None => return SignalExprResult::None,
    };

    analyze_signal_expr(expr, expr_text, imported_names, local_names)
}

/// Core signal analysis on a parsed expression.
pub fn analyze_signal_expr(
    expr: &Expression,
    expr_text: &str,
    imported_names: &BTreeSet<String>,
    local_names: &BTreeSet<String>,
) -> SignalExprResult {
    // Literals are never wrapped
    match expr {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => return SignalExprResult::None,
        _ => {}
    }

    // Bare identifier — not wrapped
    if matches!(expr, Expression::Identifier(_)) {
        return SignalExprResult::None;
    }

    // Template literals without expressions — not wrapped
    if let Expression::TemplateLiteral(tpl) = expr {
        if tpl.expressions.is_empty() {
            return SignalExprResult::None;
        }
    }

    // CallExpression — not wrapped (mutable(), signal.value(), etc.)
    if matches!(expr, Expression::CallExpression(_)) {
        return SignalExprResult::None;
    }

    // ChainExpression — optional chaining like `signal.formData?.get('username')`.
    // If the chain contains a reactive root (non-imported, non-global identifier),
    // wrap with _fnSignal. Otherwise leave as-is.
    if let Expression::ChainExpression(chain) = expr {
        let root = get_chain_root(&chain.expression);
        let is_reactive = root.as_ref().map_or(false, |name| {
            !imported_names.contains(name.as_str()) && !is_global_name(name)
        });
        if is_reactive {
            let all_deps = collect_all_deps_from_chain(&chain.expression, imported_names);
            if !all_deps.is_empty() {
                let (hoisted_fn, hoisted_str) = generate_fn_signal(expr_text, expr, &all_deps);
                return SignalExprResult::FnSignal {
                    deps: all_deps,
                    hoisted_fn,
                    hoisted_str,
                };
            }
        }
        return SignalExprResult::None;
    }

    // Arrow functions — not wrapped
    if matches!(expr, Expression::ArrowFunctionExpression(_)) {
        return SignalExprResult::None;
    }

    // MemberExpression handling
    if let Expression::StaticMemberExpression(member) = expr {
        let obj_ident = get_root_identifier_name(&member.object);
        // An identifier is potentially reactive if it's NOT imported and NOT a well-known global.
        // This includes both declared locals AND nested function params (e.g., `row` in `.map((row) => ...)`).
        // Imported names (dep.thing) and globals (globalThing.thing) are NOT reactive.
        let is_potentially_reactive = obj_ident.as_ref().map_or(true, |name| {
            !imported_names.contains(name.as_str()) && !is_global_name(name)
        });

        // signal.value → _wrapProp(signal)
        if member.property.name.as_str() == "value" && is_potentially_reactive {
            if let Some(ref obj_name) = obj_ident {
                return SignalExprResult::WrapProp {
                    code: format!("_wrapProp({})", obj_name),
                };
            }
            // Complex expression.value → fnSignal
            let roots = collect_reactive_roots(expr, imported_names, local_names);
            if !roots.is_empty() {
                let all_deps = collect_all_deps(expr, imported_names);
                let (hoisted_fn, hoisted_str) = generate_fn_signal(expr_text, expr, &all_deps);
                return SignalExprResult::FnSignal {
                    deps: all_deps,
                    hoisted_fn,
                    hoisted_str,
                };
            }
        }

        // Deep store access (store.address.city.name) → fnSignal
        if is_potentially_reactive && is_deep_store_access(expr, imported_names, local_names) {
            let roots = collect_reactive_roots(expr, imported_names, local_names);
            if !roots.is_empty() {
                let all_deps = collect_all_deps(expr, imported_names);
                let (hoisted_fn, hoisted_str) = generate_fn_signal(expr_text, expr, &all_deps);
                return SignalExprResult::FnSignal {
                    deps: all_deps,
                    hoisted_fn,
                    hoisted_str,
                };
            }
        }

        // Single-level store field access → _wrapProp(store, "field")
        if is_potentially_reactive && is_store_field_access(expr, imported_names, local_names) {
            if let Expression::Identifier(obj_id) = &member.object {
                let prop_name = member.property.name.as_str();
                return SignalExprResult::WrapProp {
                    code: format!("_wrapProp({}, \"{}\")", obj_id.name, prop_name),
                };
            }
        }

        // Other member expressions — not wrapped
        return SignalExprResult::None;
    }

    // ObjectExpression / ArrayExpression with reactive roots
    if matches!(
        expr,
        Expression::ObjectExpression(_) | Expression::ArrayExpression(_)
    ) {
        let roots = collect_reactive_roots(expr, imported_names, local_names);
        if !roots.is_empty() {
            let all_deps = collect_all_deps(expr, imported_names);
            let (hoisted_fn, hoisted_str) = generate_fn_signal(expr_text, expr, &all_deps);
            return SignalExprResult::FnSignal {
                deps: all_deps,
                hoisted_fn,
                hoisted_str,
            };
        }
        return SignalExprResult::None;
    }

    // Binary / Conditional / Logical / TemplateLiteral with reactive roots
    if matches!(
        expr,
        Expression::BinaryExpression(_)
            | Expression::ConditionalExpression(_)
            | Expression::LogicalExpression(_)
            | Expression::TemplateLiteral(_)
    ) {
        // Skip if contains unknown call
        if contains_unknown_call(expr, imported_names) {
            return SignalExprResult::None;
        }
        // Skip if contains imported reference
        if contains_imported_reference(expr, imported_names) {
            return SignalExprResult::None;
        }

        let roots = collect_reactive_roots(expr, imported_names, local_names);
        if roots.is_empty() {
            return SignalExprResult::None;
        }

        let all_deps = collect_all_deps(expr, imported_names);
        let (hoisted_fn, hoisted_str) = generate_fn_signal(expr_text, expr, &all_deps);
        return SignalExprResult::FnSignal {
            deps: all_deps,
            hoisted_fn,
            hoisted_str,
        };
    }

    SignalExprResult::None
}

/// Get the root identifier name, unwrapping TS assertions and parens.
fn get_root_identifier_name(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::ParenthesizedExpression(p) => get_root_identifier_name(&p.expression),
        Expression::TSAsExpression(ts) => get_root_identifier_name(&ts.expression),
        Expression::TSNonNullExpression(ts) => get_root_identifier_name(&ts.expression),
        Expression::TSSatisfiesExpression(ts) => get_root_identifier_name(&ts.expression),
        _ => None,
    }
}

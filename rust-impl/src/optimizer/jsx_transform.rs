// Converts JSX syntax to _jsxSorted/_jsxSplit function calls.
//
// Uses position-based string replacement: parse the full segment body,
// walk the AST to find JSXElement nodes, compute replacement strings,
// then apply replacements from end-to-start to preserve offsets.
//
// Handles:
// - Self-closing elements: `<div />` → `_jsxSorted("div", null, null, null, 3, null)`
// - Elements with text children: `<div>text</div>` → `_jsxSorted("div", null, null, "text", 3, null)`
// - Nested elements: `<div><span /></div>`
// - String literal props: `<div class="foo">` → constProps `{ class: "foo" }`
// - Expression props: `<div id={expr}>` → kept in source text in constProps
// - Arrow function wrappers: `()=>{ return <div />; }` — JSX replaced in place
// - `/*#__PURE__*/` annotation on every `_jsxSorted()` call
//
// Returns None for cases we can't handle (fragments, spread attributes, etc).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use super::signal_analysis::{self, SignalExprResult, SignalHoister};

// Thread-local key state for JSX element key generation
thread_local! {
    static JSX_KEY_PREFIX: RefCell<String> = const { RefCell::new(String::new()) };
    static JSX_KEY_COUNTER: Cell<u32> = const { Cell::new(0) };
    // Signal analysis context (set by caller, used during JSX prop processing)
    static SIGNAL_HOISTER: RefCell<Option<SignalHoister>> = const { RefCell::new(None) };
    static SIGNAL_IMPORTED_NAMES: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    static SIGNAL_LOCAL_NAMES: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    // Variables known to hold Signal values (from useSignal(), useComputed$(), etc.)
    static SIGNAL_VAR_NAMES: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
}

fn next_jsx_key() -> String {
    JSX_KEY_PREFIX.with(|prefix| {
        let prefix = prefix.borrow();
        if prefix.is_empty() {
            "null".to_string()
        } else {
            let counter = JSX_KEY_COUNTER.get();
            JSX_KEY_COUNTER.set(counter + 1);
            format!("\"{}_{counter}\"", &*prefix)
        }
    })
}

/// A JSX node found in the AST, with its source span and computed replacement.
struct JsxReplacement {
    /// Start offset in the wrapped source (relative to the wrapper prefix).
    start: u32,
    /// End offset in the wrapped source.
    end: u32,
    /// The replacement string (a `_jsxSorted(...)` call).
    replacement: String,
}

/// Transform JSX in a segment body to `_jsxSorted()` function calls.
///
/// Returns `Some(transformed)` if transformation succeeded,
/// or `None` if the body contains JSX patterns we can't handle yet.
pub fn transform_jsx_in_segment(body_text: &str) -> Option<String> {
    transform_jsx_in_segment_with_key(body_text, None)
}

/// Reset the JSX key counter for a new file. Call once before processing
/// all segments in a file so the counter increments across segments.
pub fn reset_jsx_key_counter() {
    JSX_KEY_COUNTER.set(0);
}

/// Set signal analysis context for subsequent JSX transforms.
/// Call before `transform_jsx_in_segment_with_key`.
pub fn set_signal_context(
    imported_names: BTreeSet<String>,
    local_names: BTreeSet<String>,
) {
    set_signal_context_with_vars(imported_names, local_names, BTreeSet::new());
}

/// Set signal analysis context with explicit signal variable tracking.
/// `signal_var_names` contains variables known to hold Signal values (from useSignal, etc.).
pub fn set_signal_context_with_vars(
    imported_names: BTreeSet<String>,
    local_names: BTreeSet<String>,
    signal_var_names: BTreeSet<String>,
) {
    SIGNAL_HOISTER.with(|h| *h.borrow_mut() = Some(SignalHoister::new()));
    SIGNAL_IMPORTED_NAMES.with(|n| *n.borrow_mut() = imported_names);
    SIGNAL_LOCAL_NAMES.with(|n| *n.borrow_mut() = local_names);
    SIGNAL_VAR_NAMES.with(|n| *n.borrow_mut() = signal_var_names);
}

/// Take the signal hoister after JSX transform, returning hoisted declarations.
/// Returns None if no signal context was set or no hoisted functions were generated.
pub fn take_signal_hoister() -> Option<SignalHoister> {
    SIGNAL_HOISTER.with(|h| {
        let hoister = h.borrow_mut().take();
        hoister.filter(|h| !h.is_empty())
    })
}

/// Transform JSX with an optional key prefix for generating element keys.
/// key_prefix is like "u6" (first 2 chars of base64(file_hash)).
/// The key counter is NOT reset per segment — it persists across segments
/// within a file. Call `reset_jsx_key_counter()` once per file.
pub fn transform_jsx_in_segment_with_key(body_text: &str, key_prefix: Option<&str>) -> Option<String> {
    // Set up key generation state (counter is NOT reset here)
    JSX_KEY_PREFIX.with(|p| *p.borrow_mut() = key_prefix.unwrap_or("").to_string());

    // Quick check: does this body even contain JSX?
    if !likely_contains_jsx(body_text) {
        return None;
    }

    // Wrap the body so OXC can parse it as a statement.
    // We use `const __f = ` prefix so arrow functions, JSX expressions, etc. all parse.
    let prefix = "const __f = ";
    let suffix = ";";
    let wrapped = format!("{}{}{}", prefix, body_text, suffix);
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parse_result = Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked || !parse_result.errors.is_empty() {
        return None;
    }

    // Collect all top-level JSX replacements by walking the AST.
    // We only collect the outermost JSX elements — nested ones are handled recursively
    // inside `build_jsx_replacement`.
    let mut replacements: Vec<JsxReplacement> = Vec::new();
    let prefix_len = prefix.len() as u32;

    collect_jsx_replacements(
        &parse_result.program,
        &wrapped,
        prefix_len,
        &mut replacements,
    )?;

    if replacements.is_empty() {
        return None;
    }

    // Apply replacements from end to start to preserve offsets.
    // Offsets are relative to body_text (we subtract prefix_len).
    let mut result = body_text.to_string();
    replacements.sort_by(|a, b| b.start.cmp(&a.start));

    for rep in &replacements {
        let start = (rep.start - prefix_len) as usize;
        let end = (rep.end - prefix_len) as usize;
        if start > result.len() || end > result.len() || start > end {
            return None;
        }
        result.replace_range(start..end, &rep.replacement);
    }

    Some(result)
}

/// Heuristic check for JSX-like content.
fn likely_contains_jsx(text: &str) -> bool {
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b'<' && bytes[i + 1].is_ascii_alphabetic() {
            return true;
        }
        // Also detect fragments: `<>`
        if bytes[i] == b'<' && bytes[i + 1] == b'>' {
            return true;
        }
    }
    false
}

/// Walk the program AST and collect outermost JSX element replacements.
/// Returns None if any JSX node can't be transformed (bail out entirely).
fn collect_jsx_replacements(
    program: &Program,
    source: &str,
    prefix_len: u32,
    replacements: &mut Vec<JsxReplacement>,
) -> Option<()> {
    // Walk all statements looking for expressions containing JSX.
    for stmt in &program.body {
        collect_from_statement(stmt, source, prefix_len, replacements)?;
    }
    Some(())
}

/// Recursively collect JSX replacements from a statement.
fn collect_from_statement(
    stmt: &Statement,
    source: &str,
    prefix_len: u32,
    replacements: &mut Vec<JsxReplacement>,
) -> Option<()> {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                if let Some(init) = &declarator.init {
                    collect_from_expression(init, source, prefix_len, replacements)?;
                }
            }
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                collect_from_expression(arg, source, prefix_len, replacements)?;
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            collect_from_expression(&expr_stmt.expression, source, prefix_len, replacements)?;
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_from_statement(s, source, prefix_len, replacements)?;
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_from_statement(&if_stmt.consequent, source, prefix_len, replacements)?;
            if let Some(alt) = &if_stmt.alternate {
                collect_from_statement(alt, source, prefix_len, replacements)?;
            }
        }
        _ => {}
    }
    Some(())
}

/// Recursively collect JSX replacements from an expression.
/// When we find a JSX element, we build its full replacement (including nested JSX)
/// and add it — we don't recurse into the JSX children for separate collection.
fn collect_from_expression(
    expr: &Expression,
    source: &str,
    _prefix_len: u32,
    replacements: &mut Vec<JsxReplacement>,
) -> Option<()> {
    match expr {
        Expression::JSXElement(el) => {
            let replacement_str = build_jsx_element_replacement(el, source, true)?;
            replacements.push(JsxReplacement {
                start: el.span.start,
                end: el.span.end,
                replacement: format!("/*#__PURE__*/ {}", replacement_str),
            });
        }
        Expression::JSXFragment(frag) => {
            let replacement_str = build_jsx_fragment_replacement(frag, source, true)?;
            replacements.push(JsxReplacement {
                start: frag.span.start,
                end: frag.span.end,
                replacement: format!("/*#__PURE__*/ {}", replacement_str),
            });
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_from_expression(&paren.expression, source, _prefix_len, replacements)?;
        }
        Expression::ArrowFunctionExpression(arrow) => {
            // Walk into arrow function body
            if arrow.expression {
                // Expression body: () => <div />
                if let Some(Statement::ExpressionStatement(expr_stmt)) =
                    arrow.body.statements.first()
                {
                    collect_from_expression(
                        &expr_stmt.expression,
                        source,
                        _prefix_len,
                        replacements,
                    )?;
                }
            } else {
                // Block body: () => { return <div />; }
                for stmt in &arrow.body.statements {
                    collect_from_statement(stmt, source, _prefix_len, replacements)?;
                }
            }
        }
        Expression::ConditionalExpression(cond) => {
            collect_from_expression(&cond.consequent, source, _prefix_len, replacements)?;
            collect_from_expression(&cond.alternate, source, _prefix_len, replacements)?;
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_from_expression(expr, source, _prefix_len, replacements)?;
            }
        }
        Expression::CallExpression(call) => {
            for arg in &call.arguments {
                if let Argument::SpreadElement(_) = arg {
                    // skip spreads
                } else {
                    let arg_expr = arg.as_expression()?;
                    collect_from_expression(arg_expr, source, _prefix_len, replacements)?;
                }
            }
        }
        Expression::AssignmentExpression(assign) => {
            collect_from_expression(&assign.right, source, _prefix_len, replacements)?;
        }
        _ => {
            // For other expressions (identifiers, literals, member access, etc.)
            // there's no JSX to find — just skip.
        }
    }
    Some(())
}

/// Build the `_jsxSorted(...)` or `_jsxSplit(...)` replacement string for a JSXElement.
/// This handles the element and all its children recursively.
/// `is_root` controls key generation: root elements get generated keys,
/// child elements get `null` (matching SWC behavior).
fn build_jsx_element_replacement(el: &JSXElement, source: &str, is_root: bool) -> Option<String> {
    let tag_name = get_element_tag_name(&el.opening_element.name)?;

    // Check if tag is a component (starts with uppercase) or HTML element
    let is_component = tag_name.chars().next().map_or(false, |c| c.is_uppercase());

    // Process attributes (also extracts explicit key prop)
    let attr_result = process_attributes(&el.opening_element.attributes, source, is_component)?;

    // Process children
    let children_str = process_children(&el.children, source)?;

    // Determine flags — spreads override to 0
    let flags = if attr_result.has_spread {
        0
    } else {
        compute_flags(&el.children, is_component)
    };

    // Format the tag argument
    let tag_arg = if is_component {
        tag_name.clone()
    } else {
        format!("\"{}\"", escape_jsx_string(&tag_name))
    };

    // Choose _jsxSplit vs _jsxSorted
    // _jsxSplit when: spread props present OR bind:* on a component
    let use_split = attr_result.has_spread || (is_component && attr_result.has_bind);

    let fn_name = if use_split { "_jsxSplit" } else { "_jsxSorted" };

    // Build the call: fn_name(tag, varProps, constProps, children, flags, key)
    // Explicit key from JSX takes priority over auto-generated keys.
    // Components always get auto-generated keys (they need them for reconciliation).
    // Native HTML elements only get auto-generated keys when they're at root level.
    let key = if let Some(k) = attr_result.explicit_key {
        k
    } else if is_root || is_component {
        next_jsx_key()
    } else {
        "null".to_string()
    };
    Some(format!(
        "{}({}, {}, {}, {}, {}, {})",
        fn_name,
        tag_arg,
        attr_result.var_props.as_deref().unwrap_or("null"),
        attr_result.const_props.as_deref().unwrap_or("null"),
        children_str,
        flags,
        key
    ))
}

/// Result of processing JSX attributes.
struct AttrResult {
    var_props: Option<String>,
    const_props: Option<String>,
    explicit_key: Option<String>,
    has_spread: bool,
    has_bind: bool,
}

/// Check if a prop name is a "const" prop (event handler, passive, preventdefault).
/// These always go in constProps regardless of value type.
fn is_const_prop_name(name: &str) -> bool {
    name.starts_with("q-e:")
        || name.starts_with("q-d:")
        || name.starts_with("q-w:")
        || name.starts_with("q-ep:")
        || name.starts_with("q-dp:")
        || name.starts_with("q-wp:")
        || name.starts_with("passive:")
        || name.starts_with("preventdefault:")
        // host:* event forwarding props (e.g., host:onClick$) are always const
        || name.starts_with("host:")
        // $-suffixed custom event props (e.g., custom$) are always const
        || name.ends_with('$')
}

/// Represents a single item in the ordered attribute list for spread handling.
enum AttrEntry {
    /// A regular prop: `key: value` string and whether it's a const prop
    Prop { entry: String, is_const: bool },
    /// A spread: `...expr` where expr is the source text of the spread argument.
    /// `ident` is Some if the argument is a simple identifier (for _getVarProps/_getConstProps).
    Spread { expr_text: String, ident: Option<String> },
}

fn process_attributes(
    attributes: &[JSXAttributeItem],
    source: &str,
    is_component: bool,
) -> Option<AttrResult> {
    if attributes.is_empty() {
        return Some(AttrResult {
            var_props: None,
            const_props: None,
            explicit_key: None,
            has_spread: false,
            has_bind: false,
        });
    }

    let mut has_spread = false;
    let mut has_bind = false;
    let mut explicit_key: Option<String> = None;

    // Collect ordered attribute entries (preserving source order for spread interleaving)
    let mut ordered_entries: Vec<AttrEntry> = Vec::new();

    for attr_item in attributes {
        match attr_item {
            JSXAttributeItem::Attribute(attr) => {
                let name = get_attribute_name(&attr.name)?;

                // Strip passive:eventName boolean shorthand (no handler value).
                if name.starts_with("passive:") && attr.value.is_none() {
                    continue;
                }

                // Track bind:* directives
                if name.starts_with("bind:") {
                    has_bind = true;
                }

                // Extract `key` prop
                if name == "key" {
                    match &attr.value {
                        Some(JSXAttributeValue::StringLiteral(lit)) => {
                            explicit_key = Some(format!("\"{}\"", escape_jsx_string(&lit.value)));
                        }
                        Some(JSXAttributeValue::ExpressionContainer(container)) => {
                            match &container.expression {
                                JSXExpression::Identifier(id) => {
                                    explicit_key = Some(id.name.to_string());
                                }
                                _ => {
                                    if let Some(expr) = container.expression.as_expression() {
                                        let start = expr.span().start as usize;
                                        let end = expr.span().end as usize;
                                        if start < source.len() && end <= source.len() {
                                            explicit_key = Some(source[start..end].to_string());
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Convert event handler props to q-e: format
                let prop_name = convert_event_prop_name(&name);
                let (prop_name, force_quote) = if !is_component {
                    convert_html_prop_name(&prop_name)
                } else {
                    (prop_name, false)
                };
                let force_const = is_const_prop_name(&prop_name)
                    || (is_component && prop_name.starts_with("bind:"));
                let prop_key = if force_quote {
                    format!("\"{}\"", escape_jsx_string(&prop_name))
                } else {
                    format_prop_key(&prop_name)
                };

                match &attr.value {
                    Some(JSXAttributeValue::StringLiteral(lit)) => {
                        let entry = format!(
                            "{}: \"{}\"",
                            prop_key,
                            escape_jsx_string(&lit.value)
                        );
                        ordered_entries.push(AttrEntry::Prop { entry, is_const: true });
                    }
                    Some(JSXAttributeValue::ExpressionContainer(container)) => {
                        match &container.expression {
                            JSXExpression::EmptyExpression(_) => {}
                            JSXExpression::Identifier(id) => {
                                let entry = format!("{}: {}", prop_key, &id.name);
                                // If this identifier is a known signal variable (from useSignal etc.),
                                // it always goes to constProps regardless of element type
                                let is_signal_var = SIGNAL_VAR_NAMES.with(|names| {
                                    names.borrow().contains(id.name.as_str())
                                });
                                ordered_entries.push(AttrEntry::Prop {
                                    entry,
                                    is_const: force_const || is_signal_var,
                                });
                            }
                            _ => {
                                let expr = container.expression.as_expression()?;
                                let start = expr.span().start as usize;
                                let end = expr.span().end as usize;
                                if start >= source.len() || end > source.len() {
                                    return None;
                                }
                                let expr_text = &source[start..end];

                                // Try signal analysis: _wrapProp or _fnSignal
                                match try_signal_analysis(expr, expr_text, source) {
                                    SignalExprResult::WrapProp { code } => {
                                        let entry = format!("{}: {}", prop_key, code);
                                        // Single-arg _wrapProp(signal) → always constProps
                                        // Two-arg _wrapProp(obj, "prop") → constProps on components, varProps on HTML
                                        let is_single_arg = !code.contains(',');
                                        ordered_entries.push(AttrEntry::Prop {
                                            entry,
                                            is_const: is_single_arg || is_component,
                                        });
                                    }
                                    SignalExprResult::FnSignal { deps, hoisted_fn, hoisted_str } => {
                                        // Register with hoister and emit _fnSignal call
                                        let fn_call = SIGNAL_HOISTER.with(|h| {
                                            let mut hoister = h.borrow_mut();
                                            if let Some(ref mut hoister) = *hoister {
                                                let hf_name = hoister.hoist(&hoisted_fn, &hoisted_str);
                                                let deps_str = deps.join(",\n");
                                                Some(format!("_fnSignal({}, [\n{}\n], {}_str)", hf_name, deps_str, hf_name))
                                            } else {
                                                None
                                            }
                                        });
                                        if let Some(fn_call) = fn_call {
                                            let entry = format!("{}: {}", prop_key, fn_call);
                                            // On components: _fnSignal goes to constProps
                                            // On HTML elements: _fnSignal goes to varProps
                                            ordered_entries.push(AttrEntry::Prop { entry, is_const: is_component });
                                        } else {
                                            // No hoister available, fall back to raw expression
                                            let entry = format!("{}: {}", prop_key, expr_text);
                                            ordered_entries.push(AttrEntry::Prop {
                                                entry,
                                                is_const: force_const,
                                            });
                                        }
                                    }
                                    SignalExprResult::None => {
                                        let entry = format!("{}: {}", prop_key, expr_text);
                                        ordered_entries.push(AttrEntry::Prop {
                                            entry,
                                            is_const: force_const,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Some(JSXAttributeValue::Element(_)) | Some(JSXAttributeValue::Fragment(_)) => {
                        return None;
                    }
                    None => {
                        // Boolean shorthand: `<div disabled />` → `disabled: true`
                        // Always const (matching original SWC behavior)
                        let entry = format!("{}: true", prop_key);
                        ordered_entries.push(AttrEntry::Prop { entry, is_const: true });
                    }
                }
            }
            JSXAttributeItem::SpreadAttribute(spread) => {
                has_spread = true;
                let arg = &spread.argument;
                let start = arg.span().start as usize;
                let end = arg.span().end as usize;
                if start < source.len() && end <= source.len() {
                    let expr_text = source[start..end].to_string();
                    // Check if argument is a simple identifier
                    let ident = if let Expression::Identifier(id) = arg {
                        Some(id.name.to_string())
                    } else {
                        None
                    };
                    ordered_entries.push(AttrEntry::Spread { expr_text, ident });
                } else {
                    return None;
                }
            }
        }
    }

    // If no spreads, use the original simple path (sorted varProps)
    if !has_spread {
        let mut const_props: Vec<String> = Vec::new();
        let mut var_props: Vec<String> = Vec::new();
        for entry in ordered_entries {
            if let AttrEntry::Prop { entry, is_const } = entry {
                if is_const {
                    const_props.push(entry);
                } else {
                    var_props.push(entry);
                }
            }
        }

        // SWC sorts varProps alphabetically
        var_props.sort_by(|a, b| {
            let key_a = a.split(':').next().unwrap_or("").trim().trim_matches('"');
            let key_b = b.split(':').next().unwrap_or("").trim().trim_matches('"');
            key_a.cmp(key_b)
        });

        let var_props_str = if var_props.is_empty() {
            None
        } else {
            Some(format!("{{\n{}\n}}", var_props.join(",\n")))
        };
        let const_props_str = if const_props.is_empty() {
            None
        } else {
            Some(format!("{{\n{}\n}}", const_props.join(",\n")))
        };

        return Some(AttrResult {
            var_props: var_props_str,
            const_props: const_props_str,
            explicit_key,
            has_spread,
            has_bind,
        });
    }

    // === Spread path: build interleaved varProps/constProps ===
    //
    // Based on SWC output patterns:
    // - Each identifier spread `{...x}` → `..._getVarProps(x)` in varProps,
    //   and `_getConstProps(x)` contributes to constProps
    // - Non-identifier spreads `{...expr}` → `...expr` in varProps (pass-through)
    // - Props BEFORE the last identifier spread → go into varProps (source order)
    // - String literal / boolean props AFTER the last identifier spread → constProps
    // - Dynamic / event handler props AFTER spread → varProps
    // - When multiple identifier spreads exist OR dynamic props follow a spread,
    //   _getConstProps merges into varProps and constProps becomes null

    // Find index of the last identifier spread
    let last_ident_spread_idx = ordered_entries.iter().rposition(|e| {
        matches!(e, AttrEntry::Spread { ident: Some(_), .. })
    });

    // Check if we need to merge constProps into varProps:
    // - Multiple spreads (any kind), OR
    // - Any non-const (dynamic) prop after the last identifier spread
    let total_spreads = ordered_entries.iter().filter(|e| {
        matches!(e, AttrEntry::Spread { .. })
    }).count();

    let has_dynamic_after_last_spread = if let Some(last_idx) = last_ident_spread_idx {
        ordered_entries[last_idx + 1..].iter().any(|e| {
            matches!(e, AttrEntry::Prop { is_const: false, .. })
                || matches!(e, AttrEntry::Spread { .. })
        })
    } else {
        false
    };

    let merge_const_into_var = total_spreads > 1 || has_dynamic_after_last_spread;

    let mut var_entries: Vec<String> = Vec::new();
    let mut const_entries_after_spread: Vec<String> = Vec::new();
    let mut get_const_props_idents: Vec<String> = Vec::new();

    for (idx, entry) in ordered_entries.iter().enumerate() {
        match entry {
            AttrEntry::Spread { expr_text, ident } => {
                if let Some(id) = ident {
                    var_entries.push(format!("..._getVarProps({})", id));
                    if merge_const_into_var {
                        var_entries.push(format!("..._getConstProps({})", id));
                    } else {
                        get_const_props_idents.push(id.clone());
                    }
                } else {
                    // Non-identifier spread: pass through directly
                    var_entries.push(format!("...{}", expr_text));
                }
            }
            AttrEntry::Prop { entry, is_const } => {
                // Decide if this prop goes to varProps or constProps
                let after_last_ident_spread = last_ident_spread_idx
                    .map_or(false, |li| idx > li);

                if after_last_ident_spread && *is_const && !merge_const_into_var {
                    // String literal / boolean prop after last spread → constProps
                    const_entries_after_spread.push(entry.clone());
                } else {
                    // Everything else goes into varProps
                    var_entries.push(entry.clone());
                }
            }
        }
    }

    let var_props_str = if var_entries.is_empty() {
        None
    } else {
        Some(format!("{{\n{}\n}}", var_entries.join(",\n")))
    };

    // Build constProps
    let const_props_str = if merge_const_into_var {
        // Everything is in varProps
        None
    } else if !const_entries_after_spread.is_empty() && !get_const_props_idents.is_empty() {
        // Mix: `{ ..._getConstProps(x), additionalProp: value }`
        let mut parts: Vec<String> = Vec::new();
        for id in &get_const_props_idents {
            parts.push(format!("..._getConstProps({})", id));
        }
        parts.extend(const_entries_after_spread);
        Some(format!("{{\n{}\n}}", parts.join(",\n")))
    } else if !get_const_props_idents.is_empty() {
        // Simple: just `_getConstProps(x)` (no braces)
        if get_const_props_idents.len() == 1 {
            Some(format!("_getConstProps({})", get_const_props_idents[0]))
        } else {
            let mut parts: Vec<String> = Vec::new();
            for id in &get_const_props_idents {
                parts.push(format!("..._getConstProps({})", id));
            }
            Some(format!("{{\n{}\n}}", parts.join(",\n")))
        }
    } else if !const_entries_after_spread.is_empty() {
        Some(format!("{{\n{}\n}}", const_entries_after_spread.join(",\n")))
    } else {
        None
    };

    Some(AttrResult {
        var_props: var_props_str,
        const_props: const_props_str,
        explicit_key,
        has_spread,
        has_bind,
    })
}

/// Convert event handler prop names from JSX convention to Qwik internal format.
/// `onClick$` -> `q-e:click`, `onDblClick$` -> `q-e:dblclick`,
/// `onDocumentScroll$` -> `q-e:documentscroll`, etc.
/// Also handles `on:` namespace syntax: `on:click$` -> `q-e:click`.
/// Non-event props are returned unchanged.
fn convert_event_prop_name(name: &str) -> String {
    // Handle passive:eventName → q-ep:eventName (passive event marker)
    if let Some(rest) = name.strip_prefix("passive:") {
        return format!("q-ep:{}", rest);
    }
    // Handle on:eventName$ syntax (namespaced JSX attribute)
    if let Some(rest) = name.strip_prefix("on:") {
        let event = rest.strip_suffix('$').unwrap_or(rest);
        return format!("q-e:{}", swc_normalize_event(event));
    }

    if !name.ends_with('$') {
        return name.to_string();
    }

    // Determine prefix and event part following SWC's get_event_scope_data logic.
    // SWC checks: window:on → q-w:, document:on → q-d:, on → q-e:
    // The event part starts immediately after the prefix and ends before $.
    let (prefix, event) = if let Some(rest) = name.strip_prefix("window:on") {
        ("q-w:", rest.strip_suffix('$').unwrap_or(rest))
    } else if let Some(rest) = name.strip_prefix("document:on") {
        ("q-d:", rest.strip_suffix('$').unwrap_or(rest))
    } else if let Some(rest) = name.strip_prefix("on") {
        let event = rest.strip_suffix('$').unwrap_or(rest);
        // SWC only treats this as an event if the char after "on" is uppercase or "-"
        let first_char = event.chars().next();
        if first_char.map_or(false, |c| c.is_uppercase() || c == '-') {
            ("q-e:", event)
        } else {
            return name.to_string();
        }
    } else {
        return name.to_string();
    };

    format!("{}{}", prefix, swc_normalize_event(event))
}

/// Normalize a JSX event name following SWC's normalize_jsx_event_name + create_event_name.
///
/// If the name starts with `-`, it's a case-sensitive marker: strip the dash,
/// keep original casing, then apply camelCase-to-kebab via create_event_name.
/// Otherwise, lowercase first, then apply create_event_name (which doubles dashes).
fn swc_normalize_event(name: &str) -> String {
    if name == "DOMContentLoaded" {
        return "-d-o-m-content-loaded".to_string();
    }

    let processed = if let Some(stripped) = name.strip_prefix('-') {
        // Case-sensitive marker: keep original case
        stripped.to_string()
    } else {
        name.to_lowercase()
    };

    // create_event_name: uppercase letters and dashes both produce a `-` prefix
    let mut result = String::new();
    for c in processed.chars() {
        if c.is_ascii_uppercase() || c == '-' {
            result.push('-');
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert React-style HTML prop names to their HTML equivalents.
/// Only for native HTML elements, not components.
/// Returns (converted_name, needs_quoting) - needs_quoting is true when the
/// name was converted to a JS reserved word (e.g. className -> class).
fn convert_html_prop_name(name: &str) -> (String, bool) {
    match name {
        "className" => ("class".to_string(), true),
        "htmlFor" => ("for".to_string(), true),
        _ => (name.to_string(), false),
    }
}

/// Convert camelCase or PascalCase to kebab-case.
#[allow(dead_code)]
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Format a prop key for JS output.
/// If it's a valid JS identifier, use it bare; otherwise quote it.
fn format_prop_key(name: &str) -> String {
    if is_valid_js_identifier(name) {
        name.to_string()
    } else {
        format!("\"{}\"", escape_jsx_string(name))
    }
}

/// Check if a string is a valid JS identifier (simplified).
fn is_valid_js_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Get the string name from a JSX attribute name.
fn get_attribute_name(name: &JSXAttributeName) -> Option<String> {
    match name {
        JSXAttributeName::Identifier(id) => Some(id.name.to_string()),
        JSXAttributeName::NamespacedName(ns) => {
            Some(format!("{}:{}", ns.namespace.name, ns.name.name))
        }
    }
}

/// Process JSX children into a string representation.
fn process_children(children: &[JSXChild], source: &str) -> Option<String> {
    // Filter out whitespace-only text nodes
    let significant: Vec<&JSXChild> = children.iter().filter(|c| !is_whitespace_text(c)).collect();

    if significant.is_empty() {
        return Some("null".to_string());
    }

    if significant.len() == 1 {
        return process_single_child(significant[0], source);
    }

    // Multiple children — build array
    // SWC formats each child on its own line
    let mut child_strs = Vec::new();
    for child in &significant {
        let child_str = process_child_to_code(child, source)?;
        child_strs.push(child_str);
    }
    Some(format!("[\n{}\n]", child_strs.join(",\n")))
}

/// Process a single child, returning its string representation.
fn process_single_child(child: &JSXChild, source: &str) -> Option<String> {
    match child {
        JSXChild::Text(text) => {
            let trimmed = text.value.trim();
            if trimmed.is_empty() {
                Some("null".to_string())
            } else {
                Some(format!("\"{}\"", escape_jsx_string(trimmed)))
            }
        }
        JSXChild::Element(el) => {
            let replacement = build_jsx_element_replacement(el, source, false)?;
            Some(format!("/*#__PURE__*/ {}", replacement))
        }
        JSXChild::ExpressionContainer(container) => match &container.expression {
            JSXExpression::EmptyExpression(_) => Some("null".to_string()),
            JSXExpression::Identifier(id) => Some(id.name.to_string()),
            _ => {
                let expr = container.expression.as_expression()?;
                let start = expr.span().start as usize;
                let end = expr.span().end as usize;
                if start >= source.len() || end > source.len() {
                    return None;
                }
                let expr_text = &source[start..end];
                match try_signal_analysis(expr, expr_text, source) {
                    SignalExprResult::WrapProp { code } => Some(code),
                    SignalExprResult::FnSignal { deps, hoisted_fn, hoisted_str } => {
                        let fn_call = SIGNAL_HOISTER.with(|h| {
                            let mut hoister = h.borrow_mut();
                            if let Some(ref mut hoister) = *hoister {
                                let hf_name = hoister.hoist(&hoisted_fn, &hoisted_str);
                                let deps_str = deps.join(",\n");
                                Some(format!("_fnSignal({}, [\n{}\n], {}_str)", hf_name, deps_str, hf_name))
                            } else {
                                None
                            }
                        });
                        fn_call.or_else(|| Some(expr_text.to_string()))
                    }
                    SignalExprResult::None => {
                        if let Some(transformed) = transform_jsx_in_expression(expr, expr_text, source) {
                            Some(transformed)
                        } else {
                            Some(expr_text.to_string())
                        }
                    }
                }
            }
        },
        JSXChild::Fragment(frag) => {
            let replacement = build_jsx_fragment_replacement(frag, source, false)?;
            Some(format!("/*#__PURE__*/ {}", replacement))
        }
        JSXChild::Spread(_) => None,
    }
}

/// Process a child into its code representation (for arrays).
fn process_child_to_code(child: &JSXChild, source: &str) -> Option<String> {
    match child {
        JSXChild::Text(text) => {
            let trimmed = text.value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("\"{}\"", escape_jsx_string(trimmed)))
            }
        }
        JSXChild::Element(el) => {
            let replacement = build_jsx_element_replacement(el, source, false)?;
            Some(format!("/*#__PURE__*/ {}", replacement))
        }
        JSXChild::ExpressionContainer(container) => match &container.expression {
            JSXExpression::EmptyExpression(_) => None,
            JSXExpression::Identifier(id) => Some(id.name.to_string()),
            _ => {
                let expr = container.expression.as_expression()?;
                let start = expr.span().start as usize;
                let end = expr.span().end as usize;
                if start >= source.len() || end > source.len() {
                    return None;
                }
                let expr_text = &source[start..end];
                match try_signal_analysis(expr, expr_text, source) {
                    SignalExprResult::WrapProp { code } => Some(code),
                    SignalExprResult::FnSignal { deps, hoisted_fn, hoisted_str } => {
                        let fn_call = SIGNAL_HOISTER.with(|h| {
                            let mut hoister = h.borrow_mut();
                            if let Some(ref mut hoister) = *hoister {
                                let hf_name = hoister.hoist(&hoisted_fn, &hoisted_str);
                                let deps_str = deps.join(",\n");
                                Some(format!("_fnSignal({}, [\n{}\n], {}_str)", hf_name, deps_str, hf_name))
                            } else {
                                None
                            }
                        });
                        fn_call.or_else(|| Some(expr_text.to_string()))
                    }
                    SignalExprResult::None => {
                        if let Some(transformed) = transform_jsx_in_expression(expr, expr_text, source) {
                            Some(transformed)
                        } else {
                            Some(expr_text.to_string())
                        }
                    }
                }
            }
        },
        JSXChild::Fragment(frag) => {
            let replacement = build_jsx_fragment_replacement(frag, source, false)?;
            Some(format!("/*#__PURE__*/ {}", replacement))
        }
        JSXChild::Spread(_) => None,
    }
}

/// Transform JSX inside an arbitrary expression, returning transformed source text.
/// Used for expression children like `data.map((row) => <tr>...</tr>)` where
/// the expression itself is not JSX but contains JSX in nested callbacks.
fn transform_jsx_in_expression(expr: &Expression, expr_text: &str, source: &str) -> Option<String> {
    // Quick check: does this expression text contain JSX?
    if !likely_contains_jsx(expr_text) {
        return None;
    }

    // Collect JSX replacements from within this expression
    let mut replacements: Vec<JsxReplacement> = Vec::new();
    collect_from_expression(expr, source, 0, &mut replacements)?;

    if replacements.is_empty() {
        return None;
    }

    // Apply replacements from end to start.
    // The replacements have offsets relative to the full `source` string.
    // We need to convert them to offsets relative to `expr_text`.
    let expr_start = expr.span().start as usize;
    let mut result = expr_text.to_string();
    replacements.sort_by(|a, b| b.start.cmp(&a.start));

    for rep in &replacements {
        let start = (rep.start as usize).checked_sub(expr_start)?;
        let end = (rep.end as usize).checked_sub(expr_start)?;
        if start > result.len() || end > result.len() || start > end {
            return None;
        }
        result.replace_range(start..end, &rep.replacement);
    }

    Some(result)
}

/// Analyze a JSX prop/child expression for signal wrapping.
/// Uses thread-local signal context (imported/local names) if available.
/// Falls back to simple _wrapProp heuristic when no context is set.
fn try_signal_analysis(expr: &Expression, expr_text: &str, _source: &str) -> SignalExprResult {
    // Try full signal analysis with thread-local context
    let has_context = SIGNAL_HOISTER.with(|h| h.borrow().is_some());
    if has_context {
        let result = SIGNAL_IMPORTED_NAMES.with(|imported| {
            SIGNAL_LOCAL_NAMES.with(|local| {
                let imported = imported.borrow();
                let local = local.borrow();
                signal_analysis::analyze_signal_expr(expr, expr_text, &imported, &local)
            })
        });
        return result;
    }

    // Fallback: simple _wrapProp for obj.value / obj.prop patterns
    if let Expression::StaticMemberExpression(member) = expr {
        if let Expression::Identifier(obj_id) = &member.object {
            let obj_name = obj_id.name.as_str();
            let prop_name = member.property.name.as_str();
            if prop_name == "value" {
                return SignalExprResult::WrapProp {
                    code: format!("_wrapProp({})", obj_name),
                };
            } else {
                return SignalExprResult::WrapProp {
                    code: format!("_wrapProp({}, \"{}\")", obj_name, prop_name),
                };
            }
        }
    }
    SignalExprResult::None
}

/// Compute the flags value for a JSX element.
/// In SWC output: 3 = static element (no dynamic children), 1 = has dynamic children.
fn compute_flags(children: &[JSXChild], _is_component: bool) -> u32 {
    // SWC flags encode: bit0 = static_listeners, bit1 = static_subtree.
    // Flag 3 = static listeners + static subtree (most common default)
    // Flag 1 = static listeners only (when all children are component elements)
    //
    // The rule: use flag 1 only when all significant children are component
    // elements (uppercase tag name). Otherwise use flag 3.
    let significant: Vec<&JSXChild> = children.iter().filter(|c| !is_whitespace_text(c)).collect();

    if significant.is_empty() {
        return 3; // No children
    }

    let all_component_elements = significant.iter().all(|c| {
        if let JSXChild::Element(el) = c {
            if let Some(tag) = get_element_tag_name(&el.opening_element.name) {
                return tag.chars().next().map_or(false, |ch| ch.is_uppercase());
            }
        }
        false
    });

    if all_component_elements {
        1
    } else {
        3
    }
}

/// Check if a JSXChild is a whitespace-only text node.
/// Clean JSX text content following React/SWC rules:
/// 1. Split into lines
/// 2. Trim each line
/// 3. Remove empty lines at start/end
fn is_whitespace_text(child: &JSXChild) -> bool {
    if let JSXChild::Text(text) = child {
        text.value.trim().is_empty()
    } else {
        false
    }
}

/// Build the `_jsxSorted(_Fragment, ...)` replacement string for a JSXFragment.
fn build_jsx_fragment_replacement(frag: &JSXFragment, source: &str, is_root: bool) -> Option<String> {
    // Process children - component children of fragments get auto keys
    let children_str = process_children(&frag.children, source)?;

    // Fragments use flag 1 in SWC for most cases.
    let flags = 1;

    let key = if is_root { next_jsx_key() } else { "null".to_string() };
    Some(format!(
        "_jsxSorted(_Fragment, null, null, {}, {}, {})",
        children_str, flags, key
    ))
}

/// Get the tag name string from a JSXElementName.
fn get_element_tag_name(name: &JSXElementName) -> Option<String> {
    match name {
        JSXElementName::Identifier(id) => Some(id.name.to_string()),
        JSXElementName::IdentifierReference(id) => Some(id.name.to_string()),
        JSXElementName::NamespacedName(ns) => {
            Some(format!("{}:{}", ns.namespace.name, ns.name.name))
        }
        JSXElementName::MemberExpression(member) => Some(get_member_expr_name(member)),
        JSXElementName::ThisExpression(_) => Some("this".to_string()),
    }
}

/// Build a dotted name from a JSX member expression (e.g., `Foo.Bar.Baz`).
fn get_member_expr_name(member: &JSXMemberExpression) -> String {
    let obj = match &member.object {
        JSXMemberExpressionObject::IdentifierReference(id) => id.name.to_string(),
        JSXMemberExpressionObject::MemberExpression(inner) => get_member_expr_name(inner),
        JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
    };
    format!("{}.{}", obj, member.property.name)
}


/// Escape special characters in strings for JS output.
fn escape_jsx_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Bare JSX expressions (existing tests, adapted) ===

    #[test]
    fn test_self_closing_element() {
        let result = transform_jsx_in_segment("<div />");
        assert_eq!(
            result,
            Some("/*#__PURE__*/ _jsxSorted(\"div\", null, null, null, 3, null)".to_string())
        );
    }

    #[test]
    fn test_self_closing_no_space() {
        let result = transform_jsx_in_segment("<div/>");
        assert_eq!(
            result,
            Some("/*#__PURE__*/ _jsxSorted(\"div\", null, null, null, 3, null)".to_string())
        );
    }

    #[test]
    fn test_empty_element() {
        let result = transform_jsx_in_segment("<div></div>");
        assert_eq!(
            result,
            Some("/*#__PURE__*/ _jsxSorted(\"div\", null, null, null, 3, null)".to_string())
        );
    }

    #[test]
    fn test_text_child() {
        let result = transform_jsx_in_segment("<div>Hello</div>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, null, \"Hello\", 3, null)".to_string()
            )
        );
    }

    #[test]
    fn test_text_child_with_whitespace() {
        let result = transform_jsx_in_segment("<span> Hello World </span>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"span\", null, null, \"Hello World\", 3, null)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_nested_elements() {
        let result = transform_jsx_in_segment("<div><span /></div>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, null, /*#__PURE__*/ _jsxSorted(\"span\", null, null, null, 3, null), 3, null)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_no_jsx() {
        let result = transform_jsx_in_segment("console.log('hello')");
        assert_eq!(result, None);
    }

    #[test]
    fn test_component_tag() {
        let result = transform_jsx_in_segment("<MyComponent />");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(MyComponent, null, null, null, 3, null)".to_string()
            )
        );
    }

    #[test]
    fn test_multiple_children() {
        let result = transform_jsx_in_segment("<div><span /><p /></div>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, null, [\n/*#__PURE__*/ _jsxSorted(\"span\", null, null, null, 3, null),\n/*#__PURE__*/ _jsxSorted(\"p\", null, null, null, 3, null)\n], 3, null)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_comparison_not_jsx() {
        let result = transform_jsx_in_segment("a < b");
        assert_eq!(result, None);
    }

    // === NEW: Arrow function wrappers ===

    #[test]
    fn test_arrow_expression_returning_jsx() {
        let result = transform_jsx_in_segment("()=><div />");
        assert_eq!(
            result,
            Some(
                "()=>/*#__PURE__*/ _jsxSorted(\"div\", null, null, null, 3, null)".to_string()
            )
        );
    }

    #[test]
    fn test_arrow_block_returning_jsx() {
        let result = transform_jsx_in_segment("()=>{ return <div />; }");
        assert_eq!(
            result,
            Some(
                "()=>{ return /*#__PURE__*/ _jsxSorted(\"div\", null, null, null, 3, null); }"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_arrow_with_params_returning_jsx() {
        let result = transform_jsx_in_segment("(props)=>{ return <div>text</div>; }");
        assert_eq!(
            result,
            Some(
                "(props)=>{ return /*#__PURE__*/ _jsxSorted(\"div\", null, null, \"text\", 3, null); }"
                    .to_string()
            )
        );
    }

    // === NEW: Props/attributes ===

    #[test]
    fn test_string_prop() {
        let result = transform_jsx_in_segment("<div class=\"foo\">text</div>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, {\nclass: \"foo\"\n}, \"text\", 3, null)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_multiple_string_props() {
        let result = transform_jsx_in_segment("<div class=\"foo\" id=\"bar\" />");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, {\nclass: \"foo\",\nid: \"bar\"\n}, null, 3, null)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_expression_prop() {
        // Identifier expressions go to varProps (2nd arg), not constProps (3rd arg)
        let result = transform_jsx_in_segment("<div id={expr} />");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", {\nid: expr\n}, null, null, 3, null)".to_string()
            )
        );
    }

    #[test]
    fn test_mixed_props() {
        // String literal goes to constProps (3rd), identifier to varProps (2nd)
        let result = transform_jsx_in_segment("<div class=\"foo\" id={expr} />");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", {\nid: expr\n}, {\nclass: \"foo\"\n}, null, 3, null)"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_boolean_prop() {
        let result = transform_jsx_in_segment("<input disabled />");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"input\", null, {\ndisabled: true\n}, null, 3, null)"
                    .to_string()
            )
        );
    }

    // === Props inside arrow functions ===

    #[test]
    fn test_arrow_with_props() {
        let result = transform_jsx_in_segment("()=>{ return <div class=\"foo\">text</div>; }");
        assert_eq!(
            result,
            Some(
                "()=>{ return /*#__PURE__*/ _jsxSorted(\"div\", null, {\nclass: \"foo\"\n}, \"text\", 3, null); }"
                    .to_string()
            )
        );
    }

    // === Expression children ===

    #[test]
    fn test_expression_child() {
        let result = transform_jsx_in_segment("<div>{value}</div>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, null, value, 3, null)".to_string()
            )
        );
    }

    #[test]
    fn test_expression_child_complex() {
        let result = transform_jsx_in_segment("<div>{a + b}</div>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(\"div\", null, null, a + b, 3, null)".to_string()
            )
        );
    }

    // === Spread attributes → _jsxSplit ===

    #[test]
    fn test_spread_attribute_uses_jsx_split() {
        let result = transform_jsx_in_segment("<div {...props} />");
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("_jsxSplit("), "should use _jsxSplit, got: {}", output);
        assert!(output.contains("_getVarProps(props)"), "should have _getVarProps, got: {}", output);
        assert!(output.contains("_getConstProps(props)"), "should have _getConstProps, got: {}", output);
    }

    // === Fragment still returns None ===

    #[test]
    fn test_fragment_with_child() {
        let result = transform_jsx_in_segment("<>child</>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(_Fragment, null, null, \"child\", 1, null)".to_string()
            )
        );
    }

    #[test]
    fn test_fragment_empty() {
        let result = transform_jsx_in_segment("<></>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(_Fragment, null, null, null, 1, null)".to_string()
            )
        );
    }

    #[test]
    fn test_fragment_multiple_children() {
        let result = transform_jsx_in_segment("<><div /><span /></>");
        assert_eq!(
            result,
            Some(
                "/*#__PURE__*/ _jsxSorted(_Fragment, null, null, [\n/*#__PURE__*/ _jsxSorted(\"div\", null, null, null, 3, null),\n/*#__PURE__*/ _jsxSorted(\"span\", null, null, null, 3, null)\n], 1, null)".to_string()
            )
        );
    }

    // === Real-world segment bodies ===

    #[test]
    fn test_realistic_segment_body() {
        let input = "()=>{\n    return <div class=\"container\"><span>Hello</span></div>;\n}";
        let result = transform_jsx_in_segment(input);
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.starts_with("()=>"));
        assert!(output.contains("/*#__PURE__*/"));
        assert!(output.contains("_jsxSorted(\"div\""));
        assert!(output.contains("class: \"container\""));
        assert!(output.contains("_jsxSorted(\"span\""));
    }
}

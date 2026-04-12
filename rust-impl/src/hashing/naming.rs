/// Display name and symbol name construction for Qwik segments.
///
/// Replicates the Rust optimizer's escape_sym(), register_context_name(),
/// and symbol name generation.
use super::siphash::qwik_hash;

/// Escape a string to contain only alphanumeric characters and underscores.
///
/// - Non-alphanumeric characters become underscores
/// - Leading non-alnum characters are dropped (no leading underscore)
/// - Trailing non-alnum characters are dropped (no trailing underscore)
/// - Consecutive non-alnum characters produce a single underscore
pub fn escape_sym(s: &str) -> String {
    let mut result = String::new();
    let mut pending_underscore = false;
    let mut has_content = false;

    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_underscore && has_content {
                result.push('_');
            }
            result.push(ch);
            has_content = true;
            pending_underscore = false;
        } else if has_content {
            pending_underscore = true;
        }
    }
    result
}

/// Build the full display name from a file stem and context stack.
///
/// The display name is "{fileStem}_{escapedContext}".
/// - Joins context_stack with "_"
/// - If stack is empty, uses "s_"
/// - Runs escape_sym on the joined string
/// - Prepends "_" if result starts with a digit
/// - Prepends file_stem + "_"
pub fn build_display_name(file_stem: &str, context_stack: &[&str]) -> String {
    let joined = if context_stack.is_empty() {
        "s_".to_string()
    } else {
        context_stack.join("_")
    };

    let mut escaped = escape_sym(&joined);

    // If result starts with a digit, prepend underscore
    if escaped.starts_with(|c: char| c.is_ascii_digit()) {
        escaped.insert(0, '_');
    }

    // For empty stack, escape_sym("s_") produces "s" but we want "s_"
    if context_stack.is_empty() {
        return format!("{}_s_", file_stem);
    }

    format!("{}_{}", file_stem, escaped)
}

/// Build a symbol name from a display name, scope, and relative path.
///
/// The symbol name is "{contextPortion}_{hash}" where:
/// - contextPortion is everything after "{fileStem}_" in the displayName
/// - hash is qwik_hash(scope, rel_path, contextPortion)
pub fn build_symbol_name(display_name: &str, scope: Option<&str>, rel_path: &str) -> String {
    // Extract the file stem from the rel_path to find the context portion
    let basename = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let prefix = format!("{}_", basename);

    let context_portion = if let Some(rest) = display_name.strip_prefix(&prefix) {
        rest
    } else {
        display_name
    };

    let hash = qwik_hash(scope, rel_path, context_portion);
    format!("{}_{}", context_portion, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_sym_basic() {
        assert_eq!(escape_sym("hello"), "hello");
        assert_eq!(escape_sym("hello_world"), "hello_world");
    }

    #[test]
    fn test_escape_sym_special_chars() {
        assert_eq!(escape_sym("on$click"), "on_click");
        assert_eq!(escape_sym("$hello$"), "hello");
    }

    #[test]
    fn test_escape_sym_consecutive_specials() {
        assert_eq!(escape_sym("a$$b"), "a_b");
    }

    #[test]
    fn test_escape_sym_leading_trailing() {
        assert_eq!(escape_sym("$$$hello$$$"), "hello");
    }

    #[test]
    fn test_escape_sym_s_underscore() {
        assert_eq!(escape_sym("s_"), "s");
    }

    #[test]
    fn test_build_display_name_empty_stack() {
        assert_eq!(build_display_name("test.tsx", &[]), "test.tsx_s_");
    }

    #[test]
    fn test_build_display_name_with_context() {
        assert_eq!(
            build_display_name("test.tsx", &["renderHeader1"]),
            "test.tsx_renderHeader1"
        );
    }

    #[test]
    fn test_build_display_name_nested() {
        assert_eq!(
            build_display_name("test.tsx", &["renderHeader1", "div", "onClick$"]),
            "test.tsx_renderHeader1_div_onClick"
        );
    }

    #[test]
    fn test_build_symbol_name() {
        let display = "test.tsx_renderHeader1";
        let name = build_symbol_name(display, None, "test.tsx");
        assert_eq!(name, "renderHeader1_jMxQsjbyDss");
    }

    #[test]
    fn test_build_symbol_name_nested() {
        let display = "test.tsx_renderHeader1_div_onClick";
        let name = build_symbol_name(display, None, "test.tsx");
        assert_eq!(name, "renderHeader1_div_onClick_USi8k1jUb40");
    }
}

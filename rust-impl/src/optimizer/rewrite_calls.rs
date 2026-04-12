
/// Build a QRL variable declaration.
///
/// Example output:
/// ```js
/// const q_renderHeader1_jMxQsjbyDss = /*#__PURE__*/ qrl(
///   ()=>import("./test.tsx_renderHeader1_jMxQsjbyDss"),
///   "renderHeader1_jMxQsjbyDss"
/// );
/// ```
pub fn build_qrl_declaration(
    symbol_name: &str,
    canonical_filename: &str,
    captures: &[String],
    _extension: &str,
) -> String {
    let var_name = format!("q_{}", symbol_name);
    let import_path = format!("./{}", canonical_filename);

    let captures_arg = if captures.is_empty() {
        String::new()
    } else {
        format!(", [{}]", captures.join(", "))
    };

    format!(
        "const {} = /*#__PURE__*/ qrl(()=>import(\"{}\"), \"{}\"{})",
        var_name, import_path, symbol_name, captures_arg
    )
}

/// Build a QRL variable name from a symbol name.
pub fn qrl_var_name(symbol_name: &str) -> String {
    format!("q_{}", symbol_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_qrl_declaration_no_captures() {
        let result = build_qrl_declaration(
            "renderHeader1_jMxQsjbyDss",
            "test.tsx_renderHeader1_jMxQsjbyDss",
            &[],
            "tsx",
        );
        assert_eq!(
            result,
            "const q_renderHeader1_jMxQsjbyDss = /*#__PURE__*/ qrl(()=>import(\"./test.tsx_renderHeader1_jMxQsjbyDss\"), \"renderHeader1_jMxQsjbyDss\")"
        );
    }

    #[test]
    fn test_build_qrl_declaration_with_captures() {
        let result = build_qrl_declaration(
            "onClick_abc123",
            "test.tsx_onClick_abc123",
            &["state".to_string(), "count".to_string()],
            "tsx",
        );
        assert!(result.contains("[state, count]"));
    }

    #[test]
    fn test_qrl_var_name() {
        assert_eq!(qrl_var_name("foo_abc123"), "q_foo_abc123");
    }
}

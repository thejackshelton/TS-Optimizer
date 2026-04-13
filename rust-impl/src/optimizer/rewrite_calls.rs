
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
    _captures: &[String],
    extension: &str,
) -> String {
    let var_name = format!("q_{}", symbol_name);
    let import_path = if extension.is_empty() {
        format!("./{}", canonical_filename)
    } else {
        format!("./{}.{}", canonical_filename, extension)
    };

    // SWC never puts captures in the qrl() declaration.
    // Captures are attached at the use site via `.w([vars])`.
    format!(
        "const {} = /*#__PURE__*/ qrl(()=>import(\"{}\"), \"{}\")",
        var_name, import_path, symbol_name
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
            "",
        );
        assert_eq!(
            result,
            "const q_renderHeader1_jMxQsjbyDss = /*#__PURE__*/ qrl(()=>import(\"./test.tsx_renderHeader1_jMxQsjbyDss\"), \"renderHeader1_jMxQsjbyDss\")"
        );
    }

    #[test]
    fn test_build_qrl_declaration_with_explicit_extension() {
        let result = build_qrl_declaration(
            "renderHeader1_jMxQsjbyDss",
            "test.tsx_renderHeader1_jMxQsjbyDss",
            &[],
            "js",
        );
        assert_eq!(
            result,
            "const q_renderHeader1_jMxQsjbyDss = /*#__PURE__*/ qrl(()=>import(\"./test.tsx_renderHeader1_jMxQsjbyDss.js\"), \"renderHeader1_jMxQsjbyDss\")"
        );
    }

    #[test]
    fn test_build_qrl_declaration_with_captures() {
        let result = build_qrl_declaration(
            "onClick_abc123",
            "test.tsx_onClick_abc123",
            &["state".to_string(), "count".to_string()],
            "",
        );
        // Captures are no longer included in QRL declarations (they use .w() at use site)
        assert!(!result.contains("[state, count]"));
        assert!(result.contains("qrl(()=>import(\"./test.tsx_onClick_abc123\"), \"onClick_abc123\")"));
    }

    #[test]
    fn test_qrl_var_name() {
        assert_eq!(qrl_var_name("foo_abc123"), "q_foo_abc123");
    }
}

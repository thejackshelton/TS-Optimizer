/// Emits diagnostic objects (C02, C03, C05, preventdefault-passive-check).
/// Parses/applies @qwik-disable-next-line suppression directives.
use super::types::{Diagnostic, DiagnosticCategory};

pub fn emit_c02(file: &str, message: &str) -> Diagnostic {
    Diagnostic {
        category: DiagnosticCategory::Error,
        code: "C02".to_string(),
        file: file.to_string(),
        message: message.to_string(),
        highlights: None,
        suggestions: None,
        scope: String::new(),
    }
}

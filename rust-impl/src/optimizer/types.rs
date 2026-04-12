/// API types for the Qwik optimizer.
///
/// These types define the public interface for transform_module() and related
/// functions. They must match the NAPI binding interface exactly so this Rust
/// optimizer is a drop-in replacement.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformModulesOptions {
    pub input: Vec<TransformModuleInput>,
    pub src_dir: String,
    #[serde(default)]
    pub root_dir: Option<String>,
    #[serde(default)]
    pub entry_strategy: Option<EntryStrategy>,
    #[serde(default)]
    pub minify: Option<MinifyMode>,
    #[serde(default)]
    pub source_maps: Option<bool>,
    #[serde(default)]
    pub transpile_ts: Option<bool>,
    #[serde(default)]
    pub transpile_jsx: Option<bool>,
    #[serde(default)]
    pub preserve_filenames: Option<bool>,
    #[serde(default)]
    pub explicit_extensions: Option<bool>,
    #[serde(default)]
    pub mode: Option<EmitMode>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub strip_exports: Option<Vec<String>>,
    #[serde(default)]
    pub reg_ctx_name: Option<Vec<String>>,
    #[serde(default)]
    pub strip_ctx_name: Option<Vec<String>>,
    #[serde(default)]
    pub strip_event_handlers: Option<bool>,
    #[serde(default)]
    pub is_server: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformModuleInput {
    pub path: String,
    pub code: String,
    #[serde(default)]
    pub dev_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformOutput {
    pub modules: Vec<TransformModule>,
    pub diagnostics: Vec<Diagnostic>,
    pub is_type_script: bool,
    pub is_jsx: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformModule {
    pub path: String,
    pub is_entry: bool,
    pub code: String,
    pub map: Option<String>,
    pub segment: Option<SegmentAnalysis>,
    pub orig_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Segment analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentAnalysis {
    pub origin: String,
    pub name: String,
    pub entry: Option<String>,
    pub display_name: String,
    pub hash: String,
    pub canonical_filename: String,
    pub extension: String,
    pub parent: Option<String>,
    pub ctx_kind: SegmentKind,
    pub ctx_name: String,
    pub captures: bool,
    pub loc: (u32, u32),
}

/// Extended segment metadata with optional fields for snapshot comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentMetadataInternal {
    #[serde(flatten)]
    pub base: SegmentAnalysis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_names: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Strategy and mode types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntryStrategy {
    Inline,
    Hoist,
    #[serde(rename_all = "camelCase")]
    Hook {
        #[serde(default)]
        manual: Option<std::collections::HashMap<String, String>>,
    },
    #[serde(rename_all = "camelCase")]
    Segment {
        #[serde(default)]
        manual: Option<std::collections::HashMap<String, String>>,
    },
    #[serde(rename_all = "camelCase")]
    Single {
        #[serde(default)]
        manual: Option<std::collections::HashMap<String, String>>,
    },
    #[serde(rename_all = "camelCase")]
    Component {
        #[serde(default)]
        manual: Option<std::collections::HashMap<String, String>>,
    },
    #[serde(rename_all = "camelCase")]
    Smart {
        #[serde(default)]
        manual: Option<std::collections::HashMap<String, String>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MinifyMode {
    Simplify,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmitMode {
    Dev,
    Prod,
    Lib,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentKind {
    #[serde(rename = "eventHandler")]
    EventHandler,
    #[serde(rename = "function")]
    Function,
    #[serde(rename = "jSXProp")]
    JsxProp,
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticHighlightFlat {
    pub lo: u32,
    pub hi: u32,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    pub code: String,
    pub file: String,
    pub message: String,
    pub highlights: Option<Vec<DiagnosticHighlightFlat>>,
    pub suggestions: Option<()>,
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticCategory {
    Error,
    Warning,
}

impl Default for EmitMode {
    fn default() -> Self {
        EmitMode::Prod
    }
}

impl Default for MinifyMode {
    fn default() -> Self {
        MinifyMode::None
    }
}

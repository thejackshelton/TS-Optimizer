# Architecture

**Analysis Date:** 2026-04-08

## Pattern Overview

**Overall:** Multi-layer AST transformation pipeline using OXC (Oxide) as the parsing and code generation foundation, replacing the original SWC-based implementation. The optimizer extracts Qrl-marked segments (lazy-loadable closures) from Qwik components into isolated modules.

**Key Characteristics:**
- Single-responsibility layers: parsing → AST traversal → transformation → codegen
- OXC-based AST manipulation with semantic analysis for symbol tracking
- Insta snapshot testing for deterministic output validation
- Workspace structure: core optimizer crate + Node.js NAPI binding

## Layers

**Parsing & Source (`source.rs`):**
- Purpose: Read source files and normalize them into a consistent format
- Location: `optimizer/src/source.rs`
- Contains: `Source` enum with `ScriptFile` variant holding raw text and `SourceInfo`
- Depends on: `SourceInfo` (component metadata), Language detection
- Used by: `js_lib_interface::transform_modules()` (entry point)

**Semantic Analysis (`transform.rs` - TransformGenerator):**
- Purpose: Traverse AST with semantic context to identify Qrl markers, symbol definitions, and scope relationships
- Location: `optimizer/src/transform.rs`
- Contains: `TransformGenerator` struct (maintains symbol tables, import stacks, component stacks); `OptimizedApp` (transformed body + extracted components)
- Depends on: OXC semantic builder, `TraverseCtx` for scope information
- Used by: Code transformation and component extraction pipeline

**Component Extraction (`component/` module):**
- Purpose: Model and generate Qrl component code segments with metadata
- Location: `optimizer/src/component/`
- Contains: `QrlComponent` (component definition), `Id` (unique identifier with hash/scope), `Qrl` (import path and display name), `Language` enum
- Depends on: OXC codegen for output generation
- Used by: `TransformGenerator` to emit extracted segments

**Segment Management (`segment.rs`):**
- Purpose: Build unique segment names from nested Qrl contexts, preventing collisions
- Location: `optimizer/src/segment.rs`
- Contains: `Segment` enum (Named, NamedQrl, IndexQrl), `SegmentBuilder` with name uniqueness tracking
- Depends on: HashMap for tracking name frequencies
- Used by: TransformGenerator during AST traversal for hierarchical naming

**Dead Code Removal (`dead_code.rs`):**
- Purpose: Eliminate unreferenced bindings from extracted segment modules
- Location: `optimizer/src/dead_code.rs`
- Contains: `DeadCode` trait with implementations for `FunctionBody` and `Statement`
- Depends on: OXC AST types
- Used by: Post-transform optimization

**Illegal Code Detection (`illegal_code.rs`):**
- Purpose: Identify code patterns forbidden inside Qrl scopes (e.g., capturing function/class definitions)
- Location: `optimizer/src/illegal_code.rs`
- Contains: `IllegalCodeType` enum (Class, Function), `IllegalCode` trait for detecting violations
- Depends on: OXC semantic symbol tracking
- Used by: Transform to emit `ProcessingFailure` diagnostics

**Import Cleanup (`import_clean_up.rs`):**
- Purpose: Reconcile and reorder imports, rename legacy `@builder.io` Qwik imports to `@qwik.dev`
- Location: `optimizer/src/import_clean_up.rs`
- Contains: `ImportCleanUp` struct with BTreeMap-based import tracking and rename logic
- Depends on: OXC semantic analysis
- Used by: Component code generation

**Entry Strategy (`entry_strategy.rs`):**
- Purpose: Define how extracted segments are grouped (Inline, Hoist, Single, Hook, Segment, Component, Smart)
- Location: `optimizer/src/entry_strategy.rs`
- Contains: `EntryStrategy` enum, `EntryPolicy` trait with implementations for each strategy
- Depends on: `Segment` context
- Used by: TransformGenerator to determine segment organization

**Interface/Output (`js_lib_interface.rs`):**
- Purpose: Define public API types matching the original Qwik optimizer for compatibility
- Location: `optimizer/src/js_lib_interface.rs`
- Contains: `TransformModulesOptions` (config), `TransformOutput` (results), `TransformModule`, `SegmentAnalysis`, `Diagnostic`
- Depends on: All transform layers
- Used by: NAPI binding and tests

**NAPI Binding (`napi/src/lib.rs`):**
- Purpose: Expose `transform_modules` to Node.js with async/threading support
- Location: `napi/src/lib.rs`
- Contains: Single exported function `transform_modules` using tokio `spawn_blocking`
- Depends on: qwik-optimizer crate, NAPI SDK, tokio runtime
- Used by: Node.js code calling the Rust optimizer

**Extensions (`ext/` module):**
- Purpose: Add convenience methods to OXC AST types
- Location: `optimizer/src/ext/`
- Contains: `ast_builder_ext.rs` (helper constructors), `expression_ext.rs` (expression utilities)
- Depends on: OXC AST
- Used by: Component generation and transform

## Data Flow

**Transform Pipeline (per input file):**

1. **Source Ingestion** → `Source::from_source()` reads code and creates `SourceInfo`
2. **Parsing** → OXC parser produces AST via `oxc_parser::Parser`
3. **Semantic Analysis** → `SemanticBuilder` creates scope graph and symbol table
4. **Traversal** → `TransformGenerator` visits AST nodes with `oxc_traverse::traverse_mut()`
   - Tracks symbol definitions in `symbol_by_name` HashMap
   - Maintains stack of active Qrl contexts in `segment_stack`
   - Records imports in `import_stack` (per scope)
   - Builds component list in `components` Vec
5. **Component Extraction** → For each Qrl marker found:
   - Build segment path from context stack
   - Create unique `Id` with hash and display name
   - Generate `QrlComponent` with extracted expression and imports
   - Add to `components` Vec
6. **Dead Code Removal** → Optional pass to clean extracted modules
7. **Import Cleanup** → Rename legacy Qwik imports, reorder
8. **Codegen** → OXC `Codegen` serializes modified AST back to JavaScript
9. **Output Assembly** → `OptimizationResult` collects transformed body + extracted components + diagnostics

**Error Handling Flow:**

- `IllegalCodeType` detected during traversal → wrapped in `ProcessingFailure`
- Collected in `TransformGenerator::errors` Vec
- Converted to `Diagnostic` by `js_lib_interface::error_to_diagnostic()`
- Returned alongside successful transforms in `TransformOutput`

## Key Abstractions

**Source Representation:**
- Purpose: Abstract file I/O and language detection
- Examples: `optimizer/src/source.rs` - `Source::ScriptFile` variant
- Pattern: Enum-based, holds both code and metadata

**Component Identity:**
- Purpose: Uniquely identify extracted segments across builds
- Examples: `optimizer/src/component/id.rs` - `Id` struct with hash calculation
- Pattern: Combines file path, symbol name, scope into deterministic hash using `DefaultHasher`

**Segment Hierarchy:**
- Purpose: Build qualified names from nested Qrl contexts
- Examples: `optimizer/src/segment.rs` - `Segment` enum, `SegmentBuilder`
- Pattern: Stack-based naming with collision detection via HashMap

**Qrl Markers:**
- Purpose: Represent lazy-loadable closures with metadata
- Examples: `optimizer/src/component/qrl.rs` - `Qrl` and `QrlType` enums
- Pattern: Tracks import path, display name, and marker type (standard, prefixed, indexed)

## Entry Points

**CLI/Test Entry (`js_lib_interface::transform_modules`):**
- Location: `optimizer/src/js_lib_interface.rs:216`
- Triggers: Called by tests or NAPI binding with `TransformModulesOptions`
- Responsibilities: Orchestrate per-file transforms, aggregate results, handle errors

**NAPI Entry (`napi/src/lib.rs::transform_modules`):**
- Location: `napi/src/lib.rs:19`
- Triggers: Node.js code calling exported function with serialized options
- Responsibilities: Deserialize JS object, spawn blocking task, return Promise<TransformOutput>

**Per-File Transform (`transform::transform`):**
- Location: `optimizer/src/transform.rs` (function defined around line 400+)
- Triggers: Called once per input file from `js_lib_interface::transform_modules`
- Responsibilities: Run complete pipeline on single source file, return `OptimizationResult`

## Error Handling

**Strategy:** Collect non-fatal errors during traversal, propagate with successful results

**Patterns:**

1. **Parse/Semantic Errors** → Return early via `Result<T>` with `Error` enum
   - File I/O failures, unsupported language, OXC parsing errors

2. **Transform Errors** → Accumulate in `TransformGenerator::errors` Vec
   - Illegal code patterns (function/class captures in Qrl scope)
   - Converted to `ProcessingFailure::IllegalCode` and then `Diagnostic`

3. **Error Types**:
   - `Error` enum (internal, fatal): `Generic`, `StringConversion`, `UnsupportedLanguage`, `IO`, `OxcUnknownExtension`, `IllegalCode`
   - `ProcessingFailure` enum (non-fatal): `IllegalCode`
   - `Diagnostic` struct (output): Contains category, message, scope, optional highlights/suggestions

## Cross-Cutting Concerns

**Logging:** Not actively used; conditional `debug()` macro in `TransformGenerator` with `DEBUG` const flag

**Validation:** 
- `Language::try_from()` validates source type
- `SourceInfo::new()` validates file path resolution
- `IllegalCode` trait validates Qrl body content

**Authentication:** Not applicable (CLI/library tool)

**Minification:** OXC minifier integration optional via `TransformOptions::minify` flag

**Configuration:** Via `TransformModulesOptions` struct (deserialized from JS):
- `mode`: Target environment (Dev, Test, Prod, Lib)
- `transpile_ts`, `transpile_jsx`: Language feature handling
- `minify`: Simplification mode
- `entry_strategy`: Segment grouping policy
- `strip_exports`, `strip_ctx_name`: Code cleanup directives

---

*Architecture analysis: 2026-04-08*

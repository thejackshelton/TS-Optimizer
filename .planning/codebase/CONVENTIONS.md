# Coding Conventions

**Analysis Date:** 2026-04-08

## Naming Patterns

**Files:**
- Snake case: `dead_code.rs`, `import_clean_up.rs`, `ast_builder_ext.rs`
- Module files mirror their logical grouping: `component/mod.rs` flattens submodules
- Test input files named after test functions: `test_example_1.tsx` corresponds to `test_example_1()` function

**Functions:**
- Snake case: `is_recording()`, `new_segment()`, `enter_call_expression()`, `exit_variable_declarator()`
- Visitor pattern methods follow convention: `enter_*` and `exit_*` for AST traversal hooks
- Constructor functions use `new()` pattern: `TransformGenerator::new()`, `SegmentBuilder::new()`
- Helper/private functions in same scope: `move_expression()`, `make_fq_name()`, `make_unique_segment_name()`

**Variables:**
- Snake case for all bindings: `segment_stack`, `import_stack`, `component_stack`, `jsx_key_counter`
- Single letter or short names in closures: `acc`, `c`, `f`, `s` in fold/map operations
- Abbreviations allowed in context: `ctx` for context, `expr` for expression, `stmt` for statement
- Prefix `is_` for boolean queries: `is_recording()`, `is_qrl()`, `is_dead_code()`, `is_illegal_code_in_qrl()`
- Prefix `_` for intentionally unused: `_source_info` in function parameters that aren't used

**Types:**
- PascalCase for all types: `TransformGenerator`, `QrlComponent`, `OptimizedApp`, `SegmentBuilder`
- Enum variants use PascalCase: `Named`, `NamedQrl`, `IndexQrl`
- Trait names are PascalCase: `DeadCode`, `ExpressionExt`, `AstBuilderExt`, `IllegalCode`
- Generic lifetime parameters are single quotes: `'gen`, `'a`, lowercase after quote

**Constants:**
- All caps with underscores: `DEBUG`, `DUMP_FINAL_AST`, `MARKER_SUFFIX`, `TEST_FILE`
- Set at module level: `const DEBUG: bool = true`

## Code Style

**Formatting:**
- Rust edition 2021 (`edition = "2021"` in Cargo.toml)
- Default Rust formatting (no specific `.rustfmt.toml` detected)
- Consistent use of tabs/spaces follows Rust defaults

**Linting:**
- NAPI crate enforces strict clippy: `#![deny(clippy::all)]`, `#![deny(clippy::perf)]`, `#![deny(clippy::nursery)]`
- Optimizer crate: no explicit deny rules detected
- Pragmatic `#![allow(unused)]` at module level when needed (e.g., `transform.rs`)

## Import Organization

**Order:**
1. Relative crate imports: `use crate::error::Error;`
2. External crate imports: `use oxc_parser::Parser;`, `use serde::{Deserialize, Serialize};`
3. Standard library: `use std::path::{Path, PathBuf};`
4. Type imports in same group: combined with `use` statement
5. Glob imports used selectively: `use crate::ext::*;`, `use oxc_ast::ast::*;`

**Path Aliases:**
- No path aliases configured (no `paths` in workspace config)
- Full paths used explicitly: `oxc_allocator::HashMap as OxcHashMap`
- OXC types aliased to distinguish from std: `Box as OxcBox`, `Vec as OxcVec`

**Module Flattening:**
- Submodules re-exported in parent `mod.rs` using `pub(crate) use`: `pub(crate) use component::*;`
- Pattern in `component/mod.rs`: collects and flattens submodules for cleaner interface

## Error Handling

**Patterns:**
- Custom error type: `Error` enum in `error.rs` using `thiserror` crate
- Result type alias in prelude: `type Result<T> = core::result::Result<T, Error>;`
- Error variants use `#[error]` attributes for display: `#[error("Generic error: {0}")]`
- Transparent error wrapping: `#[error(transparent)] IO(#[from] std::io::Error)`
- Processing failures tracked separately: `ProcessingFailure` enum for transform-specific issues
- Errors converted to diagnostics at API boundary in `js_lib_interface.rs`

**Error Types in Use:**
- `StringConversion(String, String)`: path/name conversion failures
- `UnsupportedLanguage(String)`: file type/language detection
- `IllegalCode(IllegalCodeType)`: code patterns forbidden in Qrl scopes
- Propagated with `?` operator throughout

## Logging

**Framework:** Console-based debugging only (no formal logging library)

**Patterns:**
- Debug mode: `const DEBUG: bool = true` controls verbose output
- Println at key points: `enter_program()`, `exit_program()`, segment pushes
- Conditional debug printing: `if DEBUG { println!(...) }`
- Dump flag for development: `const DUMP_FINAL_AST: bool = false` (disabled by default)
- Visitor methods print on entry/exit for debugging traversal

## Comments

**When to Comment:**
- File header comments: `// Defines the interface between qwik-optimizer and the qwik JS library.`
- Inline clarification for non-obvious logic: `// Never push consecutive underscores.`
- State tracking in struct fields: `/// Marks whether each JSX attribute in the stack is var (false) or const (true).`
- Section separators: `// -- Flatten` in module organization

**Documentation Comments:**
- Doc comments (`///`) used for public types and APIs: `QrlComponent`, `Id`, `Segment`
- Doc comments explain purpose, parameters (in markdown heading format), and examples
- No inline documentation for private functions
- Examples included where helpful: see `component/id.rs` with JavaScript example of component structure

**Inline Comments:**
- Sparse use of inline `//` comments
- Comments explain "why" not "what": `// TODO: Figure out how to replicate root_jsx_mode from old optimizer`
- Keep comments minimal; code should be self-documenting

## Function Design

**Size:** Functions range from 2-50 lines; larger methods (100+ lines) reserved for complex visitors like `enter_call_expression()`

**Parameters:**
- Use trait bounds for flexibility: `<T: AsRef<str>>`, `<U: AsRef<str>>`
- Generic lifetimes aligned with allocator scope: `<'gen>`, `<'a>`
- Self patterns: `&self`, `&mut self`, `mut self` determined by need
- Avoid unnecessary `&` wrapping for small types

**Return Values:**
- Consistent `Result` type for fallible operations: returns `Result<T>` for public APIs
- Option type for queries: `Option<&QrlComponent>`, `Option<String>`, `Option<IllegalCodeType>`
- Unit type `()` for side-effect operations: traverser visitor methods return `()`
- Some methods return owned values: `new()` constructors, `new_segment()` returns `Segment`

## Module Design

**Exports:**
- Public API in `lib.rs`: only critical types exported (no wholesale `pub mod`)
- Selective visibility: `pub mod component`, `pub mod source`, `pub mod js_lib_interface` (public-facing APIs)
- Private modules: `mod dead_code`, `mod segment`, `mod transform` (internal implementation)
- Prelude module pattern: `prelude.rs` exports `Result<T>` for convenience

**Barrel Files:**
- Used in `component/mod.rs`: collects submodules with `pub(crate) use`
- Keeps imports cleaner: `use crate::component::*` pulls in all types
- Alternative: direct imports still preferred in some places (e.g., `transform.rs` imports specific traits)

**Cross-Module Visibility:**
- `pub(crate)` for internal-only APIs: `pub(crate) fn render_segments()`
- Limit field access: some fields private with accessor methods
- Separation of concerns: visitor logic stays in `transform.rs`, types in separate modules

## Trait Implementation Patterns

**Extension Traits:**
- Location: `ext/` directory houses trait extensions
- Naming: `Trait` + `Ext`: `AstBuilderExt`, `ExpressionExt`
- Purpose: extend OXC types without modifying them
- Example: `ExpressionExt` adds `is_qrl_replaceable()` method to `Expression<'_>`

**Visitor Pattern:**
- Implements `Traverse<'a, ()>` from oxc_traverse
- Methods named `enter_*` and `exit_*`: correspond to AST node types
- Mutable context: `&mut TraverseCtx<'a, ()>` allows manipulation during traversal
- Self state: maintains stacks (segment_stack, import_stack, component_stack)

---

*Convention analysis: 2026-04-08*

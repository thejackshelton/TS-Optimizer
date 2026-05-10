# External Integrations

**Analysis Date:** 2026-04-08

## APIs & External Services

**Not Applicable:**
- This codebase does not integrate with external APIs or third-party services
- It is a self-contained optimizer library with no network I/O, webhooks, or service dependencies

## Data Storage

**Databases:**
- Not used - this is a stateless code transformation library

**File Storage:**
- **Local filesystem only** - reads input files via `fs::read_to_string()` and `Source::from_file()`
- No cloud storage integration (S3, GCS, Azure, etc.)
- Source files loaded directly from disk paths provided in `TransformModulesOptions`

**Caching:**
- Not applicable - no caching layer or cache provider integration
- Transformation is computed on-demand per invocation

## Authentication & Identity

**Auth Provider:**
- Not used - no authentication or authorization required
- Library operates on provided source code without identity checks

## Monitoring & Observability

**Error Tracking:**
- None configured - errors are returned as `Diagnostic` structures to the caller
- Caller (Qwik build system) responsible for error reporting
- Error types defined in `optimizer/src/error.rs` using `thiserror` crate

**Logs:**
- No logging framework - library is silent by default
- Diagnostics returned in `TransformOutput.diagnostics` vec
- Error details available via `std::error::Error` trait implementation

**Diagnostics Output:**
- `Diagnostic` structures serialized to JSON via serde
- Format: `{ rule: string, level: "error" | "warn", message: string, file: string, pos: number, range: [number, number], hints: string[] }`
- Returned to JavaScript caller in `TransformOutput` struct (see `optimizer/src/js_lib_interface.rs`)

## CI/CD & Deployment

**Hosting:**
- Not deployed as a service - distributed as:
  1. **Rust crate** - Available on crates.io as `qwik-optimizer`
  2. **NAPI binding** - Compiled to native `.node` binary for Node.js consumption
  3. **Workspace** - Source available on GitHub

**CI Pipeline:**
- None detected in codebase
- Build managed via Cargo and NAPI build system
- Tests run via `cargo test`

**Build Targets:**
- macOS (Darwin)
- Linux (x86_64, arm64)
- Windows (x86_64)
- WASM: `wasm32-unknown-unknown` target available via toolchain

## Environment Configuration

**Required env vars:**
- None - all configuration passed as structured JSON via `TransformModulesOptions` from JavaScript

**Optional env vars:**
- None detected

**Runtime Configuration:**
- Configuration is in-memory, passed from JavaScript to Rust via NAPI:
  - `src_dir`: Source directory path
  - `root_dir`: Optional root directory
  - `input`: Array of file paths and code to transform
  - `source_maps`: Whether to generate source maps
  - `minify`: Simplify or None
  - `transpile_ts`: TypeScript transpilation flag
  - `transpile_jsx`: JSX transpilation flag
  - `preserve_filenames`: Preserve original filenames in output
  - `entry_strategy`: How to group segments (Segment, Component, etc.)
  - `explicit_extensions`: Use explicit file extensions
  - `mode`: Target environment (production, development)
  - `scope`: Optional scope prefix
  - `core_module`: Optional core module path
  - `strip_exports`, `strip_ctx_name`, `reg_ctx_name`: Export stripping options
  - `strip_event_handlers`: Remove event handler code
  - `is_server`: Server-side rendering flag

**Secrets location:**
- Not applicable - no secrets required

## Webhooks & Callbacks

**Incoming:**
- Not applicable - library does not expose HTTP endpoints

**Outgoing:**
- Not applicable - library does not make outbound HTTP calls

## Inter-Process Communication

**NAPI Binding:**
- Synchronous JSON serialization/deserialization between Node.js and Rust
- Main export: `transform_modules(options: TransformModulesOptions) -> Promise<TransformOutput>`
  - Defined in `napi/src/lib.rs`
  - Async in JavaScript but uses `tokio::spawn_blocking` for CPU-intensive work
  - Thread pool scheduling managed by Tokio runtime

## AST Source Dependencies

**Parser Input:**
- **OXC Parser** processes:
  - JavaScript files (`.js`, `.mjs`, `.cjs`)
  - TypeScript files (`.ts`, `.tsx`, `.cts`, `.mts`)
  - Detected via file extension via `oxc_span::SourceType`

**Transformer Pipeline:**
- **OXC Transformer**: TypeScript and JSX transpilation (optional, configurable)
- **OXC Minifier**: Dead code elimination and simplification (optional)
- **OXC Semantic**: Symbol tracking, scope analysis

**Code Generation:**
- **OXC Codegen**: Generates JavaScript from modified AST with source map support (optional)

## Data Serialization

**Input Format:**
- JSON (via NAPI and serde_json)
- `TransformModuleInput` structure:
  - `path`: File path (string)
  - `devPath`: Optional development path (string)
  - `code`: JavaScript/TypeScript source code (string)

**Output Format:**
- JSON (via serde_json serialization)
- `TransformOutput` structure:
  - `modules`: Array of `TransformModule` objects
  - `diagnostics`: Array of `Diagnostic` objects
  - `isTypeScript`: Boolean
  - `isJsx`: Boolean

**Source Map Format:**
- Optional base64-encoded source map (generated by OXC codegen if enabled)
- Returned as string in `TransformModule.map`

---

*Integration audit: 2026-04-08*

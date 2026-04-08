# Technology Stack

**Analysis Date:** 2026-04-08

## Languages

**Primary:**
- Rust 1.94.1 - Core optimizer logic, parsing, transformation, and code generation
- JavaScript/TypeScript - Node.js binding via NAPI; test utilities

**Secondary:**
- Nix - Development environment configuration

## Runtime

**Environment:**
- Rust stable (from Nix flake via oxalica/rust-overlay)
- Node.js (via NAPI for FFI to JavaScript)
- Tokio async runtime (for async/await in NAPI binding)

**Package Manager:**
- Cargo (Rust dependency manager)
- Cargo workspace resolver v3 (workspace at `/Users/scottweaver/Projects/qwik-optimizer/`)

## Frameworks

**Core:**
- OXC (Oxide) 0.94.0 - AST parsing, transformation, codegen, minification, semantic analysis
  - `oxc_parser` - JavaScript/TypeScript parser
  - `oxc_ast` - AST node definitions
  - `oxc_codegen` - Code generation from AST
  - `oxc_traverse` - AST traversal and mutation
  - `oxc_semantic` - Semantic analysis and symbol tracking
  - `oxc_transformer` - AST transformation (TypeScript, JSX transpilation)
  - `oxc_minifier` - Code minification
  - `oxc_ast_visit` - Visitor pattern for AST traversal
  - `oxc_allocator` - Arena allocator for AST nodes
  - `oxc_span` - Source span tracking
  - `oxc_syntax` - Syntax utilities
  - `oxc_index` - Indexing utilities

**Node.js Binding:**
- NAPI 2.16.17 - Native Addon API for Node.js integration
- napi-derive 2.16.13 - Procedural macros for NAPI boilerplate
- Tokio 1.45.1 - Async runtime for CPU-intensive work scheduling

**Testing:**
- Insta 1.43.1 - Snapshot testing framework
  - Configured with YAML output format
  - Snapshots stored in `optimizer/src/snapshots/`

**Build/Dev:**
- napi-build 2.2.1 - Build helper for NAPI projects

## Key Dependencies

**Critical:**
- `serde` 1.0.219 & `serde_json` 1.0.140 - Serialization/deserialization for JS interface
  - All public types use `#[derive(Serialize, Deserialize)]` with camelCase rename
  - JSON roundtrip between Node.js and Rust
- `oxc_parser` 0.94.0 - Parses JavaScript/TypeScript to AST
- `oxc_codegen` 0.94.0 - Serializes modified AST back to code
- `oxc_traverse` 0.94.0 - AST mutation via visitor pattern

**Infrastructure:**
- `thiserror` 2.0.11 - Error type derivation and formatting
- `base64` 0.22.1 - Base64 encoding (likely for source maps)
- `pathdiff` 0.2.3 - Path difference calculation for relative paths
- `oxc-browserslist` 2.1.2 - Browser compatibility targeting
- `glob` 0.3.2 (dev) - File globbing for test discovery
- `mimalloc` 0.1.25 (Windows only) - High-performance memory allocator

## Configuration

**Environment:**
- Configuration via Nix flake: `flake.nix`
- Development environment: `.envrc` (uses direnv + Nix flake)
- Rust toolchain: stable with `rust-src` extension and `wasm32-unknown-unknown` target

**Build:**
- Workspace: `Cargo.toml` at root with resolver v3
- Core crate: `optimizer/Cargo.toml` (edition 2021)
- NAPI binding: `napi/Cargo.toml` (edition 2021, cdylib crate type)
- NAPI build: `napi-build` in `napi/` build dependencies
- Release profile: LTO enabled in NAPI crate for smaller binary

**Serde Configuration:**
- All serialization uses `#[serde(rename_all = "camelCase")]` for JavaScript compatibility
- Key types: `TransformModulesOptions` (input), `TransformOutput`, `TransformModule`, `Diagnostic`, `SegmentAnalysis`

## Platform Requirements

**Development:**
- Rust stable toolchain (via Nix flake)
- `rust-src` component for rust-analyzer
- `wasm32-unknown-unknown` target (for WASM support)
- direnv (optional, for `.envrc` integration)
- Nix (optional, for reproducible environment)

**Production:**
- Node.js (for NAPI binding consumption)
- Target platforms: macOS (Darwin), Linux, Windows
  - Windows uses MiMalloc allocator for performance

**NAPI Compilation:**
- Generates native bindings for Node.js
- Creates dynamic library (`.node` on macOS/Linux, `.dll` on Windows)
- Requires Node.js headers at build time

## Memory & Performance

**Allocators:**
- Default: Standard Rust allocator
- Windows: MiMalloc (high-performance alternative)

**Threading:**
- Tokio multi-threaded runtime in NAPI binding
- CPU-intensive transform work spawned to thread pool via `tokio::task::spawn_blocking`

**Optimization:**
- Release builds in NAPI crate use LTO (Link Time Optimization)
- OXC minifier available for code minification pass

---

*Stack analysis: 2026-04-08*

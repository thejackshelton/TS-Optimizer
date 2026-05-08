/**
 * API types for the Qwik optimizer.
 *
 * These types define the public interface for transformModule() and related
 * functions. They must match the NAPI binding interface exactly so the
 * TypeScript optimizer is a drop-in replacement for the SWC optimizer.
 *
 * Source: Qwik optimizer types.ts (verified from GitHub + research)
 */

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/**
 * Options for the `transformModule` batch transform (Qwik optimizer entry).
 *
 * Shape matches the historical SWC/NAPI binding so callers can swap implementations.
 * Some fields exist for that contract but are not read yet by this TypeScript
 * optimizer (see each property).
 */
export interface TransformModulesOptions {
  /** Source modules to transform (path + source per file). */
  input: TransformModuleInput[];

  /**
   * Absolute path to the application source root. Used with each input's
   * `path` to compute module-relative paths (e.g. for outputs and diagnostics).
   */
  srcDir: string;

  /**
   * Optional project root for path normalization in the full Qwik toolchain.
   * **Not used** by this implementation today; kept for NAPI parity.
   */
  rootDir?: string;

  /**
   * How lazy boundaries / QRL entry chunks are laid out (`inline`, `hoist`,
   * `hook`, `segment`, etc.).
   * @defaultValue `{ type: 'smart' }`
   */
  entryStrategy?: EntryStrategy;

  /**
   * Post-transform simplification level for emitted parent code.
   * Passed through to the parent-module rewrite step.
   */
  minify?: MinifyMode;

  /**
   * Whether to emit source maps. **Not wired** in this implementation yet
   * (output `map` fields are currently `null`); kept for NAPI parity.
   */
  sourceMaps?: boolean;

  /**
   * When `true`, strip TypeScript syntax (and related optimizer behavior such
   * as enum handling). When omitted or `false`, TypeScript is preserved except
   * where JSX transpilation implies extension changes.
   */
  transpileTs?: boolean;

  /**
   * When `false`, skip JSX→JS transform and signal-aware JSX handling.
   * When omitted or `undefined`, JSX transpilation is **enabled** (matches
   * historical default-on behavior).
   */
  transpileJsx?: boolean;

  /**
   * Preserve original filenames in emitted artifact paths/metadata where the
   * Rust optimizer does. **Not used** by this implementation yet; NAPI parity.
   */
  preserveFilenames?: boolean;

  /**
   * When `true`, relative imports to the parent module include explicit file
   * extensions (e.g. `./foo.js` vs `./foo`).
   */
  explicitExtensions?: boolean;

  /**
   * Build flavor: affects prod symbol hashing (`prod`), dev paths (`dev`/`hmr`),
   * library emit (`lib`), etc.
   * @defaultValue `'prod'`
   */
  mode?: EmitMode;

  /**
   * Optional scope string folded into QRL hashing / extraction disambiguation
   * (same role as in the upstream optimizer).
   */
  scope?: string;

  /**
   * Export names to strip from the rewritten parent module (client/server split).
   */
  stripExports?: string[];

  /**
   * Context-name patterns used when registering or rewriting stripped regions
   * (passed to parent rewrite / codegen when applicable).
   */
  regCtxName?: string[];

  /** Context names to strip in emitted parent code when strip rules apply. */
  stripCtxName?: string[];

  /** Strip JSX event props/handlers when building server-only or stripped graphs. */
  stripEventHandlers?: boolean;

  /**
   * Server build: drives constant substitution (e.g. `import.meta` / env-like
   * replacements) during parent rewrite.
   */
  isServer?: boolean;
}

export interface TransformModuleInput {
  path: string;
  code: string;
  devPath?: string;
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

export interface TransformOutput {
  modules: TransformModule[];
  diagnostics: Diagnostic[];
  isTypeScript: boolean;
  isJsx: boolean;
}

export interface TransformModule {
  path: string;
  isEntry: boolean;
  code: string;
  map: string | null;
  segment: SegmentAnalysis | null;
  origPath: string | null;
}

// ---------------------------------------------------------------------------
// Segment analysis
// ---------------------------------------------------------------------------

export interface SegmentAnalysis {
  origin: string;
  name: string;
  entry: string | null;
  displayName: string;
  hash: string;
  canonicalFilename: string;
  extension: string;
  parent: string | null;
  ctxKind: 'eventHandler' | 'function' | 'jSXProp';
  ctxName: string;
  captures: boolean;
  loc: [number, number];
}

/**
 * Internal metadata extending SegmentAnalysis with optional fields
 * used for snapshot comparison compatibility.
 *
 * paramNames and captureNames appear in snapshot metadata but are not
 * part of the public API type.
 */
export interface SegmentMetadataInternal extends SegmentAnalysis {
  paramNames?: string[];
  captureNames?: string[];
}

// ---------------------------------------------------------------------------
// Strategy and mode types
// ---------------------------------------------------------------------------

export type EntryStrategy =
  | { type: 'inline' }
  | { type: 'hoist' }
  | { type: 'hook'; manual?: Record<string, string> }
  | { type: 'segment'; manual?: Record<string, string> }
  | { type: 'single'; manual?: Record<string, string> }
  | { type: 'component'; manual?: Record<string, string> }
  | { type: 'smart'; manual?: Record<string, string> };

export type MinifyMode = 'simplify' | 'none';
export type EmitMode = 'dev' | 'prod' | 'lib' | 'hmr';

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

export interface DiagnosticHighlightFlat {
  lo: number;
  hi: number;
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

export interface Diagnostic {
  category: 'error' | 'warning';
  code: string;
  file: string;
  message: string;
  highlights: DiagnosticHighlightFlat[] | null;
  suggestions: null;
  scope: string;
}

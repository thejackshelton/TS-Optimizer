/**
 * Shared path string helpers for optimizer transforms.
 *
 * Exposes `parsePath`/`PathData` mirroring Rust's `parse_path` field-for-field
 * (swc-reference-only/parse.rs:922-961). `parsePath` is computed once per input
 * and threaded read-only through the optimizer pipeline; downstream code reads
 * its fields rather than recomputing path strings. Low-level helpers
 * (getBasename, getDirectory, getFileStem, getExtension, stripExtension,
 * normalizePath, isRelativePathInsideBase, computeParentModulePath) remain for
 * scope-out callers.
 */
import { basename, dirname, extname, join, normalize, normalizeString, parse, relative } from 'pathe';

/** Determine file extension from a path string. */
export function getExtension(filePath: string): string {
  return extname(normalizePath(filePath));
}

/** Strip file extension from a path string. */
export function stripExtension(filePath: string): string {
  const normalized = normalizePath(filePath);
  const extension = extname(normalized);
  if (!extension) return normalized;
  return normalized.slice(0, -extension.length);
}

/** Get the basename component from a slash-delimited path string. */
export function getBasename(filePath: string): string {
  return basename(normalizePath(filePath));
}

/** Get the directory component from a slash-delimited path string. */
export function getDirectory(filePath: string): string {
  const dir = dirname(normalizePath(filePath));
  return dir === '.' ? '' : dir;
}

/** Get the basename without its extension. */
export function getFileStem(filePath: string): string {
  return stripExtension(getBasename(filePath));
}

/**
 * Compute relative path from srcDir. If path doesn't start with srcDir,
 * returns the path as-is (normalized).
 */
export function computeRelPath(inputPath: string, srcDir: string): string {
  const normInput = normalizePath(inputPath);
  const normSrc = normalizePath(srcDir);

  if (normSrc === '.' || normSrc === '' || normSrc === './') {
    return normInput;
  }

  const rel = relative(normSrc, normInput);
  if (rel !== '' && rel !== '.' && rel !== '..' && !rel.startsWith('../')) {
    return rel;
  }

  return normInput;
}

/**
 * Parsed path data mirroring Rust's `PathData` struct (swc-reference-only/parse.rs:922-930).
 * Computed once per input by `parsePath` and threaded read-only through the optimizer pipeline.
 * All fields use forward-slash separators; `extension` has NO leading dot (matches Rust `path.extension()`).
 */
// PATH-01: Mirrors Rust parse_path output (swc-reference-only/parse.rs:922-930).
export interface PathData {
  readonly absPath: string;
  readonly relPath: string;
  readonly absDir: string;
  readonly relDir: string;
  readonly fileStem: string;
  readonly extension: string;
  readonly fileName: string;
}

/**
 * Parse a path string into Rust-compatible PathData.
 *
 * Mirrors Rust `parse_path` (swc-reference-only/parse.rs:932-961) field-for-field.
 * `relPath` is the input after backslash→forward-slash conversion ONLY — it preserves
 * leading './' to match Rust's `Path::to_slash_lossy()`. `absPath` is normalized
 * via pathe.normalize, which collapses './' and '..' segments (mirrors Rust normalize_path).
 */
export function parsePath(src: string, baseDir: string): PathData {
  // PATH-03: rel_path stores input verbatim (only backslash → forward-slash).
  // Do NOT call pathe.normalize here — it would strip leading './' and break
  // dev-mode _jsxSorted fileName fields (RESEARCH §Pitfall 1).
  // CLAUDE.md exception: raw RegExp used for single-character backslash-conversion
  // only — magic-regexp would obscure intent for this trivial case.
  const relPath = src.replace(/\\/g, '/');

  // PATH-02: abs_path = normalize_path(base_dir.join(path)).
  // pathe.normalize collapses './' and '..' (mirrors Rust normalize_path).
  const absPath = normalize(join(baseDir, relPath));

  // PATH-04, PATH-05: dir components. '.' → '' to match Rust Path::parent() empty PathBuf
  // and the existing getDirectory convention above.
  const parsedAbs = parse(absPath);
  const absDir = parsedAbs.dir === '.' ? '' : parsedAbs.dir;

  const parsedRel = parse(relPath);
  const relDir = parsedRel.dir === '.' ? '' : parsedRel.dir;

  // PATH-06: file_stem = basename without LAST extension (NOT all dots).
  // pathe.parse(...).name gives this directly; for 'foo/.qwik.mjs', .name === '.qwik'.
  const fileStem = parsedRel.name;

  // PATH-07: extension = last extension WITHOUT leading dot (Rust path.extension()).
  // pathe.parse(...).ext returns '.mjs'; strip the leading '.'.
  const extension = parsedRel.ext.startsWith('.') ? parsedRel.ext.slice(1) : parsedRel.ext;

  // PATH-08: file_name = basename WITH extension.
  const fileName = parsedRel.base;

  return { absPath, relPath, absDir, relDir, fileStem, extension, fileName };
}

/** Normalize a path string to use forward slashes. */
export function normalizePath(filePath: string): string {
  return normalize(filePath);
}

/**
 * Check whether a relative import path stays within the srcDir-relative tree
 * when resolved from a file's relative path.
 */
export function isRelativePathInsideBase(relativePath: string, importerPath: string): boolean {
  if (!relativePath.startsWith('.')) return false;

  const importerDir = getDirectory(normalizePath(importerPath));
  const combined = importerDir ? `${importerDir}/${relativePath}` : relativePath;
  const normalized = normalizeString(normalizePath(combined), true);

  return normalized !== '..' && !normalized.startsWith('../');
}

/**
 * Compute the parent module path for segment imports back to the parent module.
 * Segments are always emitted in the same directory as the parent file,
 * so we use only the basename (no directory component), prefixed with "./".
 */
export function computeParentModulePath(
  relPath: string,
  explicitExtensions?: boolean,
): string {
  const basename = getBasename(relPath);
  if (explicitExtensions) {
    return './' + basename;
  }
  return './' + stripExtension(basename);
}

/**
 * Compute the output file extension for QRL imports based on transpilation settings
 * and source-file kind. Verbatim port of Rust's match expression at
 * swc-reference-only/parse.rs:225-232. Returns no-dot form ('mjs', 'js', 'jsx', 'ts').
 *
 * The (transpile_ts, transpile_jsx, is_type_script, is_jsx) tuple maps to:
 *   (true,  true,  _,    _)    -> 'js'
 *   (true,  false, _,    true) -> 'jsx'
 *   (true,  false, _,    false)-> 'js'
 *   (false, true,  true, _)    -> 'ts'
 *   (false, true,  false,_)    -> 'js'
 *   (false, false, _,    _)    -> pathData.extension verbatim
 */
export function computeOutputExtension(
  pathData: PathData,
  transpileTs?: boolean,
  transpileJsx?: boolean,
  isTypeScript?: boolean,
  isJsx?: boolean,
): string {
  // Cross-plan compatibility: transform/index.ts:402 still calls this with a raw extension
  // string (e.g. '.tsx') under the legacy 3-arg shape. Plan 01-03 migrates that caller to
  // the new signature; until then, fall back to legacy dot-prefixed semantics so D-06
  // (>= 178 convergence) holds at the Plan 01-01 commit boundary.
  if (typeof pathData === 'string') {
    const sourceExt = pathData;
    if (transpileTs) return '.js';
    if (transpileJsx) return '.ts';
    return sourceExt;
  }

  const tt = !!transpileTs;
  const tj = !!transpileJsx;
  const its = !!isTypeScript;
  const ijx = !!isJsx;

  // PATH-MATRIX-1: (true, true, _, _) → 'js'
  if (tt && tj) return 'js';
  // PATH-MATRIX-2: (true, false, _, true) → 'jsx'
  if (tt && !tj && ijx) return 'jsx';
  // PATH-MATRIX-3: (true, false, _, false) → 'js'
  if (tt && !tj && !ijx) return 'js';
  // PATH-MATRIX-4: (false, true, true, _) → 'ts'
  if (!tt && tj && its) return 'ts';
  // PATH-MATRIX-5: (false, true, false, _) → 'js'
  if (!tt && tj && !its) return 'js';
  // PATH-MATRIX-6: (false, false, _, _) → pathData.extension
  return pathData.extension;
}

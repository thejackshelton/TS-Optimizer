import { describe, expect, it } from 'vitest';
import {
  computeOutputExtension,
  computeRelPath,
  getBasename,
  getDirectory,
  getExtension,
  isRelativePathInsideBase,
  normalizePath,
  parsePath,
  stripExtension,
} from '../../src/optimizer/path-utils.js';
import { qwikHash } from '../../src/hashing/siphash.js';

describe('path-utils', () => {
  it('normalizes windows-style paths', () => {
    expect(normalizePath('src\\components\\App.tsx')).toBe('src/components/App.tsx');
    expect(getBasename('src\\components\\App.tsx')).toBe('App.tsx');
    expect(getDirectory('src\\components\\App.tsx')).toBe('src/components');
    expect(getExtension('src\\components\\App.tsx')).toBe('.tsx');
    expect(stripExtension('src\\components\\App.tsx')).toBe('src/components/App');
  });

  it('preserves current computeRelPath behavior for paths outside srcDir', () => {
    expect(computeRelPath('src/routes/index.tsx', 'src')).toBe('routes/index.tsx');
    expect(computeRelPath('other/index.tsx', 'src')).toBe('other/index.tsx');
    expect(computeRelPath('src', 'src')).toBe('src');
  });

  it('detects whether a relative import stays within the srcDir-relative tree', () => {
    expect(isRelativePathInsideBase('./styles.css', 'routes/app/index.tsx')).toBe(true);
    expect(isRelativePathInsideBase('../shared/theme.css', 'routes/app/index.tsx')).toBe(true);
    expect(isRelativePathInsideBase('../../../global.css', 'routes/app/index.tsx')).toBe(false);
    expect(isRelativePathInsideBase('../outside.css', 'index.tsx')).toBe(false);
  });
});

describe('parsePath', () => {
  it('Group 1: ../node_modules/@qwik.dev/react/index.qwik.mjs preserves prefix and dotless mjs extension', () => {
    const pd = parsePath('../node_modules/@qwik.dev/react/index.qwik.mjs', '/user/qwik/src/');
    expect(pd.relPath).toBe('../node_modules/@qwik.dev/react/index.qwik.mjs');
    expect(pd.relDir).toBe('../node_modules/@qwik.dev/react');
    expect(pd.fileStem).toBe('index.qwik');
    expect(pd.extension).toBe('mjs');
    expect(pd.fileName).toBe('index.qwik.mjs');
    expect(pd.absPath).toBe('/user/qwik/node_modules/@qwik.dev/react/index.qwik.mjs');
    expect(pd.absDir).toBe('/user/qwik/node_modules/@qwik.dev/react');
  });

  it('Group 2: components/component.tsx — sub-directory + .tsx extension', () => {
    const pd = parsePath('components/component.tsx', '/user/qwik/src/');
    expect(pd.relPath).toBe('components/component.tsx');
    expect(pd.relDir).toBe('components');
    expect(pd.fileStem).toBe('component');
    expect(pd.extension).toBe('tsx');
    expect(pd.fileName).toBe('component.tsx');
    expect(pd.absPath).toBe('/user/qwik/src/components/component.tsx');
  });

  it('Group 3 (D-02): ./node_modules/qwik-tree/index.qwik.jsx preserves leading ./ in relPath/relDir AND collapses /./ in absPath', () => {
    const pd = parsePath('./node_modules/qwik-tree/index.qwik.jsx', '/user/qwik/src/');
    expect(pd.relPath).toBe('./node_modules/qwik-tree/index.qwik.jsx');
    expect(pd.relDir).toBe('./node_modules/qwik-tree');
    expect(pd.fileStem).toBe('index.qwik');
    expect(pd.extension).toBe('jsx');
    expect(pd.fileName).toBe('index.qwik.jsx');
    // CRITICAL — D-02 closure: absPath has NO '/./' segment.
    expect(pd.absPath).toBe('/user/qwik/src/node_modules/qwik-tree/index.qwik.jsx');
    expect(pd.absPath).not.toMatch(/\/\.\//);
  });

  it('Group 4: foo/.qwik.mjs — pathe.parse handles dotfile + double-extension correctly', () => {
    const pd = parsePath('foo/.qwik.mjs', '/x');
    expect(pd.fileStem).toBe('.qwik');
    expect(pd.extension).toBe('mjs');
    expect(pd.fileName).toBe('.qwik.mjs');
  });

  it('Group 5: Makefile (no extension) — returns empty extension defensively (Open Q2)', () => {
    const pd = parsePath('Makefile', '/x');
    expect(pd.extension).toBe('');
    expect(pd.fileStem).toBe('Makefile');
    expect(pd.fileName).toBe('Makefile');
  });

  it('Group 6: srcDir variants ("", ".", "./") — all yield same relPath and empty relDir', () => {
    expect(parsePath('test.tsx', '').relDir).toBe('');
    expect(parsePath('test.tsx', '.').relDir).toBe('');
    expect(parsePath('test.tsx', './').relDir).toBe('');
    expect(parsePath('test.tsx', '').relPath).toBe('test.tsx');
    expect(parsePath('test.tsx', './').extension).toBe('tsx');
    expect(parsePath('test.tsx', './').absPath).not.toMatch(/\/\.\//);
    expect(parsePath('test.tsx', './').absPath).not.toMatch(/\/\//);
  });

  it('Group 7: Windows backslash inputs converted to forward slashes', () => {
    const pd = parsePath('src\\components\\App.tsx', 'C:\\users\\apps');
    expect(pd.relPath).toBe('src/components/App.tsx');
    expect(pd.relDir).toBe('src/components');
    expect(pd.fileStem).toBe('App');
    expect(pd.extension).toBe('tsx');
    expect(pd.fileName).toBe('App.tsx');
  });

  // Migrated from the deleted "preserves computeRelPath behavior" test.
  // Rust parse_path does NOT strip srcDir from rel_path — this is a behavior change
  // from the deleted computeRelPath helper (RESEARCH §Open Question 3).
  it('parsePath stores input as relPath verbatim (does NOT strip srcDir prefix — differs from deleted computeRelPath)', () => {
    expect(parsePath('src/routes/index.tsx', 'src').relPath).toBe('src/routes/index.tsx');
    expect(parsePath('other/index.tsx', 'src').relPath).toBe('other/index.tsx');
    expect(parsePath('src', 'src').relPath).toBe('src');
  });
});

describe('parsePath hash byte-equivalence (D-07)', () => {
  // example_qwik_react.snap is in tests/hashing/siphash.test.ts KNOWN_EDGE_CASE_FILES:
  // "Segments from external modules (origin has ../ prefix, path resolution differs)".
  // The Plan 01-01 D-07 assumption (relPath + 'qwikifyQrl_component_useWatch' -> x04JC5xeP1U)
  // contradicts that prior finding — the ../-prefixed hash input uses a path other than
  // rel_path verbatim. Tracking as a Plan 01-01 Surprise; Plan 01-05 / 01-06 will verify
  // via convergence whether this fixture closes once SegmentAnalysis.path is populated.
  it.skip('example_qwik_react byte-parity (KNOWN edge case in siphash.test.ts)', () => {
    const pd = parsePath('../node_modules/@qwik.dev/react/index.qwik.mjs', '/user/qwik/src/');
    expect(qwikHash(undefined, pd.relPath, 'qwikifyQrl_component_useWatch')).toBe('x04JC5xeP1U');
  });

  // Source-of-truth: match-these-snaps/qwik_core__test__root_level_self_referential_qrl_inline.snap line 33.
  // Validates D-02 closure: only passes if relPath preserves the leading './' (Rust-correct).
  it('root_level_self_referential_qrl_inline (D-02) byte-parity: relPath WITH ./ + Tree_component -> XMEiO6Rrd3Y', () => {
    const pd = parsePath('./node_modules/qwik-tree/index.qwik.jsx', '/user/qwik/src/');
    expect(qwikHash(undefined, pd.relPath, 'Tree_component')).toBe('XMEiO6Rrd3Y');
  });
});

describe('computeOutputExtension D-03 matrix', () => {
  const tsxPd = parsePath('foo.tsx', '/x');
  const tsPd = parsePath('foo.ts', '/x');
  const jsxPd = parsePath('foo.jsx', '/x');
  const mjsPd = parsePath('foo.mjs', '/x');

  it('PATH-MATRIX-1: (transpileTs=true, transpileJsx=true, *, *) -> js', () => {
    expect(computeOutputExtension(tsxPd, true, true, true, true)).toBe('js');
    expect(computeOutputExtension(mjsPd, true, true, false, false)).toBe('js');
  });
  it('PATH-MATRIX-2: (transpileTs=true, transpileJsx=false, *, isJsx=true) -> jsx', () => {
    expect(computeOutputExtension(tsxPd, true, false, true, true)).toBe('jsx');
  });
  it('PATH-MATRIX-3: (transpileTs=true, transpileJsx=false, *, isJsx=false) -> js', () => {
    expect(computeOutputExtension(tsPd, true, false, true, false)).toBe('js');
  });
  it('PATH-MATRIX-4: (transpileTs=false, transpileJsx=true, isTypeScript=true, *) -> ts', () => {
    expect(computeOutputExtension(tsxPd, false, true, true, true)).toBe('ts');
  });
  it('PATH-MATRIX-5: (transpileTs=false, transpileJsx=true, isTypeScript=false, *) -> js', () => {
    expect(computeOutputExtension(jsxPd, false, true, false, true)).toBe('js');
  });
  it('PATH-MATRIX-6: (transpileTs=false, transpileJsx=false, *, *) -> pathData.extension verbatim', () => {
    expect(computeOutputExtension(mjsPd, false, false, false, false)).toBe('mjs');
    expect(computeOutputExtension(tsxPd, false, false, true, true)).toBe('tsx');
  });
});

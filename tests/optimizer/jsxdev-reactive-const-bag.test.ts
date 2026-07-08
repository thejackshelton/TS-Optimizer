import { describe, expect, test } from 'vitest';

import { transformModule } from '../../src/index.js';
import { mkFilePath, mkSourceText } from '../../src/optimizer/types/brands.js';

function transform(code: string): string {
  const result = transformModule({
    srcDir: mkFilePath('/proj/src'),
    input: [{ path: mkFilePath('/proj/src/c.tsx'), code: mkSourceText(code) }],
    entryStrategy: { type: 'hoist' },
    minify: 'simplify',
    transpileTs: true,
    transpileJsx: true,
    explicitExtensions: true,
    preserveFilenames: true,
    mode: 'dev',
    isServer: true,
  });
  return result.modules.map((m) => m.code).join('\n');
}

const HEADER = `import { jsxDEV as _jsxDEV } from "@qwik.dev/core/jsx-dev-runtime";
import { component$, useSignal } from '@qwik.dev/core';
`;

describe('pre-transformed `_jsxDEV` prop var/const bag classification', () => {
  test('a bare signal read is wrapped and placed in the const-props bag', () => {
    const out = transform(`${HEADER}
export const C = component$(() => {
  const label = useSignal('x');
  return _jsxDEV("div", { title: label.value }, undefined, false, undefined, this);
});
`);
    // varProps (2nd arg) is null; the wrapped read is in constProps (3rd arg).
    expect(out).toMatch(/_jsxSorted\("div",\s*null,\s*\{\s*title:\s*_wrapProp\(label\)\s*\}/);
  });

  test('a hoistable reactive expr becomes _fnSignal in the const-props bag', () => {
    const out = transform(`${HEADER}
export const C = component$(() => {
  const open = useSignal(false);
  return _jsxDEV("div", { tabIndex: open.value ? -1 : 0 }, undefined, false, undefined, this);
});
`);
    expect(out).toMatch(/_jsxSorted\("div",\s*null,\s*\{\s*tabIndex:\s*_fnSignal\(/);
  });

  test('a non-reactive dynamic prop stays in the var-props bag', () => {
    const out = transform(`${HEADER}
export const C = component$(() => {
  const cls = Math.random() > 0.5 ? 'a' : 'b';
  return _jsxDEV("div", { class: cls }, undefined, false, undefined, this);
});
`);
    // `cls` is a plain (non-signal) local — kept in the var bag (2nd arg),
    // const bag (3rd arg) is null.
    expect(out).toMatch(/_jsxSorted\("div",\s*\{\s*class:\s*cls\s*\},\s*null/);
  });

  test('the pre-analysed [_IMMUTABLE] peer-tool marker stays in the var bag', () => {
    // Peer tools (qwikify$) emit `[_IMMUTABLE]: [...]` alongside the props it
    // marks immutable. That marker must not be reclassified into the const
    // bag — it belongs where the tool placed it.
    const out = transform(`import { jsxDEV as _jsxDEV } from "@qwik.dev/core/jsx-dev-runtime";
import { component$, _IMMUTABLE } from '@qwik.dev/core';
export const C = component$((props) => {
  return _jsxDEV("div", { foo: props.foo, [_IMMUTABLE]: ["foo"] }, undefined, false, undefined, this);
});
`);
    expect(out).toContain('[_IMMUTABLE]');
    // both foo and the marker sit in the var bag (2nd arg), const bag null.
    expect(out).toMatch(/_jsxSorted\("div",\s*\{[^}]*\[_IMMUTABLE\][^}]*\},\s*null/);
  });

  test('member access on the closure param is wrapped even with no reactive binding', () => {
    // A bare `$((node) => …)` segment reads `node.id` / `node.label` — the
    // param is props-like, so the read must be `_wrapProp`'d despite the body
    // declaring no `useSignal`/`useStore`. Otherwise the raw read subscribes
    // the host during SSR.
    const out = transform(`import { jsxDEV as _jsxDEV } from "@qwik.dev/core/jsx-dev-runtime";
import { $ } from '@qwik.dev/core';
import { TreeLeaf } from './leaf';
export const renderItem = $((node) => {
  return _jsxDEV(TreeLeaf, { href: node.id, label: node.label }, void 0, false, undefined, this);
});
`);
    expect(out).toMatch(/href:\s*_wrapProp\(node,\s*"id"\)/);
    expect(out).toMatch(/label:\s*_wrapProp\(node,\s*"label"\)/);
  });
});

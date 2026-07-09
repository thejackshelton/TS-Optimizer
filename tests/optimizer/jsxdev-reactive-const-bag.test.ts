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

  test('a $-suffixed member read (handler ref) is left raw, not _wrapProp', () => {
    // `props.onChange$` is a QRL/handler reference, not a reactive value — it
    // must stay raw (a reactive `props.title` beside it still wraps).
    const out = transform(`import { jsxDEV as _jsxDEV } from "@qwik.dev/core/jsx-dev-runtime";
import { component$ } from '@qwik.dev/core';
import { Inner } from './inner';
export const C = component$((props) => {
  return _jsxDEV(Inner, { onChange$: props.onChange$, title: props.title }, void 0, false, undefined, this);
});
`);
    expect(out).toMatch(/onChange\$:\s*props\.onChange\$/);
    expect(out).not.toMatch(/_wrapProp\(props,\s*"onChange\$"\)/);
    expect(out).toMatch(/title:\s*_wrapProp\(props,\s*"title"\)/);
  });

  test('a spread on a component element emits _jsxSplit with _getVarProps/_getConstProps', () => {
    // A props-forwarding wrapper (`<Inner {...props} />`) must split the
    // spread into _getVarProps/_getConstProps under _jsxSplit so the runtime
    // classifies the forwarded keys — a _jsxSorted with `...props` in the var
    // bag would make every forwarded prop host-reactive (SSR over-subscription).
    const out = transform(`import { jsxDEV as _jsxDEV } from "@qwik.dev/core/jsx-dev-runtime";
import { component$ } from '@qwik.dev/core';
import { Inner } from './inner';
export const Wrapper = component$((props) => {
  return _jsxDEV(Inner, { tabIndex: -1, ...props, id: "x" }, void 0, false, undefined, this);
});
`);
    expect(out).toContain('_jsxSplit(Inner,');
    expect(out).toMatch(/\.\.\._getVarProps\(props\)/);
    expect(out).toMatch(/\.\.\._getConstProps\(props\)/);
    // spread expanded in place: tabIndex before, id after — all one bag, const null.
    expect(out).toMatch(/_jsxSplit\(Inner,\s*\{[^}]*tabIndex[^}]*_getVarProps\(props\)[^}]*_getConstProps\(props\)[^}]*id:[^}]*\},\s*null/);
  });
});

import type { Analyzer, Module } from "yuku-analyzer";
import type { Symbol as Binding } from "yuku-analyzer";
import type { Identifier, JSXOpeningElement, MemberExpression, Node } from "@yuku-toolchain/types";
import { type FunctionNode, is, isFunctionExpr, isHostTag, tagName, unwrap } from "./ast.js";

/** A failed analysis step, carrying a human-readable reason. */
export interface Failure {
  fail: string;
}

/** A function a JSX element resolves to, identified by its binding symbol. */
export interface Component {
  module: Module;
  symbol: Binding;
  fn: FunctionNode;
  name: string;
}

/** A prop received by destructuring, like `function C({ onPress }) {}`. */
export interface DestructuredBinding {
  kind: "destructured";
  key: Identifier;
  shorthand: boolean;
  uses: Node[];
}

/** A prop read off the props object, like `function C(props) { props.onPress }`. */
export interface MemberBinding {
  kind: "member";
  uses: MemberExpression[];
}

/** How a component receives one prop, with every use site of it. */
export type PropBinding = DestructuredBinding | MemberBinding;

/** The component a binding defines, a function declaration or `const X = () => ...`. */
export function componentFromSymbol(module: Module, symbol: Binding): Component | null {
  const decl = symbol.declarations[0];
  if (!decl) return null;
  if (is(decl, "FunctionDeclaration")) return { module, symbol, fn: decl, name: symbol.name };
  if (!is(decl, "Identifier")) return null;
  const parent = module.parentOf(decl);
  if (is(parent, "FunctionDeclaration")) return { module, symbol, fn: parent, name: symbol.name };
  if (is(parent, "VariableDeclarator") && parent.id === decl) {
    const init = unwrap(parent.init);
    if (init && isFunctionExpr(init)) return { module, symbol, fn: init, name: symbol.name };
  }
  return null;
}

/** Resolve a JSX element to "host", a Component (across files), or null when unprovable. */
export function resolveElement(
  module: Module,
  opening: JSXOpeningElement,
): Component | "host" | null {
  const name = tagName(opening);
  if (!name) return null;
  if (isHostTag(name)) return "host";
  const def = module.symbolOf(opening.name)?.definition();
  if (!def?.symbol) return null;
  return componentFromSymbol(def.module, def.symbol);
}

/** The component owning `fn` as its body, `function X` or `const X = fn`. */
export function componentOfFn(module: Module, fn: FunctionNode): Component | null {
  if (is(fn, "FunctionDeclaration") && fn.id) {
    const symbol = module.symbolOf(fn.id);
    return symbol ? { module, symbol, fn, name: symbol.name } : null;
  }
  const parent = module.parentOf(fn);
  if (is(parent, "VariableDeclarator") && is(parent.id, "Identifier")) {
    const symbol = module.symbolOf(parent.id);
    return symbol ? { module, symbol, fn, name: symbol.name } : null;
  }
  return null;
}

/**
 * Whether a component-shaped binding is safe to treat as a component. True when
 * it returns JSX and every use of it is a render or a re-export, never a plain
 * call or a value passed around.
 */
export function isComponent(analyzer: Analyzer, comp: Component): boolean {
  let rendered = false;
  for (const { module, reference } of analyzer.referencesOf(comp.symbol)) {
    const parent = module.parentOf(reference.node);
    if (is(parent, "JSXOpeningElement") || is(parent, "JSXClosingElement")) {
      rendered = true;
    } else if (
      !is(parent, "ImportSpecifier") &&
      !is(parent, "ImportDefaultSpecifier") &&
      !is(parent, "ExportSpecifier")
    ) {
      return false;
    }
  }
  return rendered || containsJsx(comp.module, comp.fn);
}

/** Whether a function's body contains any JSX. */
export function containsJsx(module: Module, fn: Node): boolean {
  let found = false;
  module.walk(
    {
      JSXElement(_node, c) {
        found = true;
        c.stop();
      },
      JSXFragment(_node, c) {
        found = true;
        c.stop();
      },
    },
    fn,
  );
  return found;
}

export function findPropBinding(component: Component, prop: string): PropBinding | Failure {
  const param = component.fn.params[0];
  if (!param) return { fail: `prop '${prop}' is never used` };

  if (is(param, "ObjectPattern")) {
    for (const property of param.properties) {
      if (
        !is(property, "Property") ||
        property.computed ||
        !is(property.key, "Identifier") ||
        property.key.name !== prop
      ) {
        continue;
      }
      if (!is(property.value, "Identifier"))
        return { fail: `unsupported pattern for prop '${prop}'` };
      const symbol = component.module.symbolOf(property.value);
      if (!symbol) return { fail: `could not resolve binding for prop '${prop}'` };
      return {
        kind: "destructured",
        key: property.key,
        shorthand: property.shorthand === true,
        uses: symbol.references.map((ref) => ref.node),
      };
    }
    if (param.properties.some((p) => is(p, "RestElement"))) {
      return { fail: `prop '${prop}' may be consumed through a rest element` };
    }
    return { fail: `prop '${prop}' is never used` };
  }

  if (is(param, "Identifier")) {
    const symbol = component.module.symbolOf(param);
    if (!symbol) return { fail: "could not resolve props parameter" };
    const uses: MemberExpression[] = [];
    for (const ref of symbol.references) {
      const member = component.module.parentOf(ref.node);
      if (
        !is(member, "MemberExpression") ||
        member.computed ||
        member.object !== ref.node ||
        !is(member.property, "Identifier")
      ) {
        return { fail: "props object escapes the component" };
      }
      if (member.property.name === prop) uses.push(member);
    }
    if (uses.length === 0) return { fail: `prop '${prop}' is never used` };
    return { kind: "member", uses };
  }

  return { fail: "unsupported props parameter pattern" };
}

import type { Analyzer, Module } from "yuku-analyzer";
import type { JSXAttribute, JSXOpeningElement, Node } from "@yuku-toolchain/types";
import {
  attrName,
  attrValue,
  is,
  isEventAttr,
  isFunctionExpr,
  isFunctionNode,
  isHostTag,
  isWrapper,
  tagName,
} from "../../core/ast.js";
import {
  type Component,
  type Failure,
  type PropBinding,
  componentOfFn,
  findPropBinding,
  resolveElement,
} from "../../core/component.js";
import { validateCaptures } from "../../core/captures.js";

/** A JSX attribute that will get a `$` suffix. */
export interface AttrRef {
  module: Module;
  attr: JSXAttribute;
}

/** Names one prop on one component, the thing the tracer walks. */
export interface PropRef {
  component: Component;
  prop: string;
}

/** A prop the tracer has resolved, carrying how its component receives it. */
interface TracedProp {
  component: Component;
  prop: string;
  binding: PropBinding;
}

/**
 * Everything marked together to turn one set of forwarded handlers into QRLs.
 * It holds the prop bindings to rename, the JSX attributes that get `$`, and the
 * inline closures to report. Renaming any one forces the rest, so the group is
 * one all-or-nothing unit.
 */
export interface HandlerGroup {
  props: TracedProp[];
  attrs: AttrRef[];
  closures: AttrRef[];
}

/** A JSX element that renders a component. */
interface Callsite {
  module: Module;
  opening: JSXOpeningElement;
}

/** Where a prop use goes, onto a host DOM event or forwarded to another prop. */
interface ToHostEvent {
  hostEvent: AttrRef;
}
interface ToProp {
  prop: PropRef;
}
type Destination = Failure | ToHostEvent | ToProp;

const PASSTHROUGH_PARENTS = new Set([
  "JSXClosingElement",
  "ImportSpecifier",
  "ImportDefaultSpecifier",
  "ExportSpecifier",
]);

export class HandlerTracer {
  readonly #analyzer: Analyzer;

  constructor(analyzer: Analyzer) {
    this.#analyzer = analyzer;
  }

  /** Prove `start` forwards to a host DOM event, gathering everything to mark. */
  trace(start: PropRef): HandlerGroup | Failure {
    const group: HandlerGroup = { props: [], attrs: [], closures: [] };
    const queue: PropRef[] = [start];
    const seen = new Set<string>();
    let reachedHostEvent = false;

    while (queue.length > 0) {
      const { component, prop } = queue.shift()!;
      const key = propKey(component, prop);
      if (seen.has(key)) continue;
      seen.add(key);

      const binding = findPropBinding(component, prop);
      if ("fail" in binding) return binding;
      group.props.push({ component, prop, binding });

      // Inside the component, each use of the prop must reach a host event or
      // forward into another component's prop.
      for (const use of binding.uses) {
        const destination = this.#destinationOf(component.module, use);
        if ("fail" in destination) return destination;
        if ("hostEvent" in destination) {
          group.attrs.push(destination.hostEvent);
          reachedHostEvent = true;
        } else {
          queue.push(destination.prop);
        }
      }

      // Outside the component, each callsite feeding the prop must be an inline
      // closure with safe captures, or a pass-through of another prop.
      const callsites = this.#callsites(component);
      if (!Array.isArray(callsites)) return callsites;
      for (const { module, opening } of callsites) {
        if (opening.attributes.some((a) => is(a, "JSXSpreadAttribute"))) {
          return { fail: `<${component.name}> spreads props at ${locationOf(module, opening)}` };
        }
        for (const attr of opening.attributes) {
          if (!is(attr, "JSXAttribute") || attrName(attr) !== prop) continue;
          const expr = attrValue(attr);
          if (!expr) return { fail: `${prop} is not an expression at ${locationOf(module, attr)}` };
          if (isFunctionExpr(expr)) {
            const reason = validateCaptures(module, expr);
            if (reason) return { fail: `${locationOf(module, attr)}: ${reason}` };
            group.attrs.push({ module, attr });
            group.closures.push({ module, attr });
          } else {
            const forwarded = forwardedProp(module, expr);
            if (!forwarded) return { fail: `unprovable value at ${locationOf(module, attr)}` };
            queue.push(forwarded);
            group.attrs.push({ module, attr });
          }
        }
      }
    }

    if (!reachedHostEvent) {
      return { fail: `${start.component.name}.${start.prop} never reaches a host event` };
    }
    return group;
  }

  #destinationOf(module: Module, use: Node): Destination {
    let node = use;
    let parent = module.parentOf(node);
    while (parent && isWrapper(parent)) {
      node = parent;
      parent = module.parentOf(parent);
    }

    if (is(parent, "CallExpression")) {
      const reason = parent.callee === node ? "called during render" : "passed to a function call";
      return { fail: `prop is ${reason}` };
    }
    if (is(parent, "ConditionalExpression") || is(parent, "LogicalExpression")) {
      return { fail: "prop is forwarded conditionally" };
    }
    if (!is(parent, "JSXExpressionContainer")) {
      return { fail: `prop is used outside JSX at ${locationOf(module, use)}` };
    }

    const attr = module.parentOf(parent);
    if (!is(attr, "JSXAttribute")) return { fail: `prop is rendered as content at ${locationOf(module, use)}` };
    const opening = module.parentOf(attr);
    if (!is(opening, "JSXOpeningElement")) return { fail: `unsupported JSX target at ${locationOf(module, use)}` };

    const name = attrName(attr);
    const tag = tagName(opening);
    if (!name || !tag) return { fail: `unsupported JSX target at ${locationOf(module, use)}` };

    if (isHostTag(tag)) {
      if (isEventAttr(name)) return { hostEvent: { module, attr } };
      return { fail: `passed to non-event attribute '${name}' on <${tag}>` };
    }
    const target = resolveElement(module, opening);
    if (target === "host" || !target) return { fail: `cannot resolve <${tag}>` };
    return { prop: { component: target, prop: name } };
  }

  /** Every JSX element rendering this component, unless one lets it escape. */
  #callsites(component: Component): Callsite[] | Failure {
    const sites: Callsite[] = [];
    for (const { module, reference } of this.#analyzer.referencesOf(component.symbol)) {
      const parent = module.parentOf(reference.node);
      if (is(parent, "JSXOpeningElement")) {
        sites.push({ module, opening: parent });
      } else if (!parent || !PASSTHROUGH_PARENTS.has(parent.type)) {
        return { fail: `<${component.name}> escapes at ${locationOf(module, reference.node)}` };
      }
    }
    return sites;
  }
}

function propKey(component: Component, prop: string): string {
  return `${component.module.path}#${component.symbol.id}#${prop}`;
}

/** When `expr` reads the enclosing component's own prop, the prop it forwards. */
function forwardedProp(module: Module, expr: Node): PropRef | null {
  if (is(expr, "Identifier")) {
    const decl = module.symbolOf(expr)?.declarations[0];
    if (!is(decl, "Identifier")) return null;
    const property = module.parentOf(decl);
    if (!is(property, "Property") || property.computed || !is(property.key, "Identifier")) return null;
    const pattern = module.parentOf(property);
    if (!is(pattern, "ObjectPattern")) return null;
    const fn = module.parentOf(pattern);
    if (!fn || !isFunctionNode(fn) || fn.params[0] !== pattern) return null;
    const component = componentOfFn(module, fn);
    return component ? { component, prop: property.key.name } : null;
  }

  if (is(expr, "MemberExpression") && !expr.computed && is(expr.object, "Identifier") && is(expr.property, "Identifier")) {
    const decl = module.symbolOf(expr.object)?.declarations[0];
    if (!is(decl, "Identifier")) return null;
    const fn = module.parentOf(decl);
    if (!fn || !isFunctionNode(fn) || fn.params[0] !== decl) return null;
    const component = componentOfFn(module, fn);
    return component ? { component, prop: expr.property.name } : null;
  }

  return null;
}

function locationOf(module: Module, node: Node): string {
  return `${module.path}:${module.locOf(node.start).line}`;
}

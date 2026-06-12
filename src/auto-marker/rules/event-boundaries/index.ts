import type { Module } from "yuku-analyzer";
import type { JSXAttribute } from "@yuku-toolchain/types";
import { attrName, attrValue, is, isEventAttr, isFunctionExpr, isHostTag, tagName } from "../../core/ast.js";
import { validateCaptures } from "../../core/captures.js";
import { resolveElement } from "../../core/component.js";
import { type MarkContext, siteOf } from "../../core/context.js";
import type { Rule } from "../rule.js";
import { type HandlerGroup, HandlerTracer } from "./forwarding.js";

/**
 * Marks inline closures that end up as host DOM event handlers, written directly
 * on a host element like `<button onClick={...}>` or forwarded as a prop through
 * components until they reach one. A forwarded handler's whole path is renamed
 * together. Anything unprovable stays eager and is reported.
 */
export const eventBoundaryRule: Rule = {
  name: "event-boundaries",
  run(ctx) {
    const tracer = new HandlerTracer(ctx.analyzer);
    const handled = new Set<string>();

    for (const module of ctx.modules()) {
      for (const attr of module.findAll("JSXAttribute")) {
        const prop = attrName(attr);
        const expr = attrValue(attr);
        if (!prop || prop.endsWith("$") || !expr || !isFunctionExpr(expr)) continue;
        if (handled.has(attrKey(module, attr))) continue;

        const opening = module.parentOf(attr);
        if (!is(opening, "JSXOpeningElement")) continue;
        const tag = tagName(opening);
        if (!tag) continue;
        const site = siteOf(module, attr, prop);

        // Direct case, a closure on a host element's own event attribute.
        if (isHostTag(tag)) {
          if (!isEventAttr(prop)) continue;
          handled.add(attrKey(module, attr));
          const reason = validateCaptures(module, expr);
          if (reason) {
            ctx.skip(site, reason);
          } else {
            ctx.mark(site);
            renameAttr(ctx, module, attr);
          }
          continue;
        }

        // Forwarded case, prove the prop forwards through components to a host event.
        const component = resolveElement(module, opening);
        if (!component || component === "host") {
          handled.add(attrKey(module, attr));
          ctx.skip(site, `cannot resolve component <${tag}>`);
          continue;
        }
        const group = tracer.trace({ component, prop });
        if ("fail" in group) {
          handled.add(attrKey(module, attr));
          ctx.skip(site, group.fail);
        } else {
          markGroup(ctx, handled, group);
        }
      }
    }
  },
};

function attrKey(module: Module, attr: JSXAttribute): string {
  return `${module.path}:${attr.start}`;
}

function renameAttr(ctx: MarkContext, module: Module, attr: JSXAttribute): void {
  if (!is(attr.name, "JSXIdentifier")) return;
  const { start, end, name } = attr.name;
  ctx.edit(module, { start, end, text: `${name}$` });
}

/** Apply a proven group. Rename every prop binding, host event, and feed, then report each closure. */
function markGroup(ctx: MarkContext, handled: Set<string>, group: HandlerGroup): void {
  for (const { binding, component, prop } of group.props) {
    if (binding.kind === "destructured") {
      const { key } = binding;
      const text = binding.shorthand ? `${prop}$: ${prop}` : `${prop}$`;
      ctx.edit(component.module, { start: key.start, end: key.end, text });
    } else {
      for (const member of binding.uses) {
        if (!is(member.property, "Identifier")) continue;
        const { start, end } = member.property;
        ctx.edit(component.module, { start, end, text: `${prop}$` });
      }
    }
  }

  for (const { module, attr } of group.attrs) renameAttr(ctx, module, attr);

  for (const { module, attr } of group.closures) {
    const key = attrKey(module, attr);
    if (handled.has(key)) continue;
    handled.add(key);
    ctx.mark(siteOf(module, attr, attrName(attr) ?? ""));
  }
}

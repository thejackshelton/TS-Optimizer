import type { Module } from "yuku-analyzer";
import { type FunctionNode, is } from "../../core/ast.js";
import { type Component, componentOfFn, isComponent } from "../../core/component.js";
import { type MarkContext, siteOf } from "../../core/context.js";
import { ensureNamedImport } from "../../core/imports.js";
import { QWIK_CORE } from "../../core/qwik.js";
import type { Rule } from "../rule.js";

/**
 * Wraps module-level components in `component$`.
 */
export const componentRule: Rule = {
  name: "components",
  run(ctx) {
    for (const module of ctx.modules()) {
      for (const fn of topLevelFunctions(module)) {
        const comp = componentOfFn(module, fn);
        if (!comp || comp.symbol.scope.kind !== "module") continue;
        if (!isComponent(ctx.analyzer, comp)) continue;

        wrap(ctx, module, comp);
        ensureNamedImport(ctx, module, "component$", QWIK_CORE);
        ctx.mark(siteOf(module, comp.fn, comp.name));
      }
    }
  },
};

/** Every top-level function. */
function topLevelFunctions(module: Module): FunctionNode[] {
  const fns: FunctionNode[] = [];
  module.walk({
    FunctionDeclaration(node, c) {
      fns.push(node);
      c.skip();
    },
    ArrowFunctionExpression(node, c) {
      fns.push(node);
      c.skip();
    },
    FunctionExpression(node, c) {
      fns.push(node);
      c.skip();
    },
  });
  return fns;
}

/** Wrap the function value in `component$(...)`, turning a declaration into a const. */
function wrap(ctx: MarkContext, module: Module, comp: Component): void {
  const { fn } = comp;
  const open = is(fn, "FunctionDeclaration") ? `const ${comp.name} = component$(` : "component$(";
  ctx.edit(module, { start: fn.start, end: fn.start, text: open });
  ctx.edit(module, { start: fn.end, end: fn.end, text: ")" });
}

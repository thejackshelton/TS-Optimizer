import type { Module } from "yuku-analyzer";
import { is } from "./ast.js";
import type { MarkContext } from "./context.js";

/**
 * Make a named import available in a module. Extends an existing import from the
 * same source, or adds a fresh line at the top. A no-op when the name is already
 * imported. Shared by any rule that emits code referencing a runtime helper.
 */
export function ensureNamedImport(
  ctx: MarkContext,
  module: Module,
  name: string,
  source: string,
): void {
  for (const imp of module.imports) {
    if (imp.name === name && imp.specifier === source) return;
  }
  for (const decl of module.findAll("ImportDeclaration")) {
    if (decl.source.value !== source) continue;
    const named = decl.specifiers.find((s) => is(s, "ImportSpecifier"));
    if (named) {
      ctx.edit(module, { start: named.start, end: named.start, text: `${name}, ` });
      return;
    }
  }
  ctx.edit(module, { start: 0, end: 0, text: `import { ${name} } from "${source}";\n` });
}

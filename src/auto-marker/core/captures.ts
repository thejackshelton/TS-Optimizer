import { SymbolFlags } from "yuku-analyzer";
import type { Module, Symbol as Binding } from "yuku-analyzer";
import type { Node } from "@yuku-toolchain/types";
import { is, unwrap } from "./ast.js";

/**
 * Conservative serializability policy. Returns the reason a closure cannot be
 * extracted, or null when every captured binding is one the optimizer can
 * restore across the boundary.
 */
export function validateCaptures(module: Module, fn: Node): string | null {
  for (const { symbol, isWritten } of module.capturesOf(fn)) {
    const reason = unsafeReason(module, symbol, isWritten);
    if (reason) return reason;
  }
  return null;
}

function unsafeReason(module: Module, symbol: Binding, isWritten: boolean): string | null {
  const name = `'${symbol.name}'`;

  if (isWritten) return `captured ${name} is reassigned inside the closure`;

  // Imports and module-scope bindings need no serialization. The optimizer
  // re-imports, moves, or re-exports them into the segment.
  if (symbol.has(SymbolFlags.Import) || symbol.scope.kind === "module") return null;

  if (symbol.has(SymbolFlags.Function | SymbolFlags.Class)) {
    return `captured ${name} is a local function or class`;
  }

  // Props serialize by Qwik's contract, and so do `use*()` state primitives.
  if (symbol.has(SymbolFlags.Parameter)) return null;
  if (symbol.has(SymbolFlags.Const) && isHookResult(module, symbol)) return null;

  return `captured ${name} is not provably serializable`;
}

function isHookResult(module: Module, symbol: Binding): boolean {
  const decl = symbol.declarations[0];
  if (!decl) return false;

  const declarator = module.parentOf(decl);
  if (!is(declarator, "VariableDeclarator")) return false;

  const init = unwrap(declarator.init);
  return is(init, "CallExpression") && is(init.callee, "Identifier") && /^use[A-Z]/.test(init.callee.name);
}

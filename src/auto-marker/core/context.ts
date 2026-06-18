import type { Analyzer, Module } from "yuku-analyzer";
import type { Node } from "@yuku-toolchain/types";
import { type Edit, applyEdits } from "./edits.js";
import type { CompiledFile, CompileResult, Decision, Site } from "../types.js";

/** Build a report Site for a node, labelled with the name being marked. */
export function siteOf(module: Module, node: Node, prop: string): Site {
  return { path: module.path, line: module.locOf(node.start).line, prop };
}

/**
 * The shared scratchpad rules write to. A rule records the edits it wants and
 * the decisions it makes, then the compiler runs every rule against one context
 * and `build()`s the final result. Rules never touch files directly.
 */
export class MarkContext {
  readonly analyzer: Analyzer;
  readonly #edits = new Map<string, Map<string, Edit>>();
  readonly #decisions: Decision[] = [];

  constructor(analyzer: Analyzer) {
    this.analyzer = analyzer;
  }

  /** Every module, in deterministic (sorted-path) order. */
  modules(): Module[] {
    return [...this.analyzer.modules.keys()].sort().map((path) => this.analyzer.modules.get(path)!);
  }

  /** Record a text edit. Identical spans collapse, so rules can be idempotent. */
  edit(module: Module, edit: Edit): void {
    let fileEdits = this.#edits.get(module.path);
    if (!fileEdits) this.#edits.set(module.path, (fileEdits = new Map()));
    fileEdits.set(`${edit.start}:${edit.end}`, edit);
  }

  mark(site: Site): void {
    this.#decisions.push({ kind: "marked", site });
  }

  skip(site: Site, reason: string): void {
    this.#decisions.push({ kind: "skipped", site, reason });
  }

  /** Apply every collected edit to its source and assemble the result. */
  build(): CompileResult {
    const files = new Map<string, CompiledFile>();
    for (const module of this.modules()) {
      const edits = [...(this.#edits.get(module.path)?.values() ?? [])];
      files.set(
        module.path,
        edits.length
          ? { code: applyEdits(module.source, edits), changed: true }
          : { code: module.source, changed: false },
      );
    }
    this.#decisions.sort(
      (a, b) =>
        a.site.path.localeCompare(b.site.path) ||
        a.site.line - b.site.line ||
        a.site.prop.localeCompare(b.site.prop),
    );
    return { files, decisions: this.#decisions };
  }
}

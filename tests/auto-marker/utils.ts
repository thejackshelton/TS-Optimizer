import { Analyzer } from "yuku-analyzer";
import { MarkContext } from "../../src/auto-marker/core/context.js";
import { eventBoundaryRule } from "../../src/auto-marker/rules/event-boundaries/index.js";
import type { Rule } from "../../src/auto-marker/rules/rule.js";
import type { Decision } from "../../src/auto-marker/types.js";

export type Files = Record<string, string>;

export interface Project {
  original: Files;
  marked: Map<string, string>;
  decisions: Decision[];
}

/** Run a chosen rule set over in-memory files. Defaults to the event rule. */
export function createProject(files: Files, rules: readonly Rule[] = [eventBoundaryRule]): Project {
  const analyzer = new Analyzer();
  for (const [path, code] of Object.entries(files)) analyzer.addFile(path, code);

  const ctx = new MarkContext(analyzer);
  for (const rule of rules) rule.run(ctx);
  const { files: out, decisions } = ctx.build();

  return {
    original: files,
    marked: new Map([...out].map(([path, file]) => [path, file.code])),
    decisions,
  };
}

/** A readable snapshot of what a compile decided and changed. */
export function report(project: Project): string {
  const decisions = project.decisions.map((d) =>
    d.kind === "marked"
      ? `mark  ${d.site.path}:${d.site.line} ${d.site.prop}`
      : `skip  ${d.site.path}:${d.site.line} ${d.site.prop}  (${d.reason})`,
  );
  const changed = [...project.marked]
    .filter(([path, code]) => code !== project.original[path])
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, code]) => `\n${path}\n${code}`);
  return [...decisions, ...changed].join("\n");
}

/** Re-mark a project's own output. Used to assert nothing changes the second time. */
export function recompile(project: Project, rules: readonly Rule[] = [eventBoundaryRule]): Project {
  return createProject(Object.fromEntries(project.marked), rules);
}

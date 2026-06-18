import type { MarkContext } from "../core/context.js";

/**
 * One auto-marking capability. A rule scans the project through the context and
 * records its edits and decisions there. Rules stay independent, each owning a
 * node kind like JSX attributes or hook calls, so a new capability is a new rule
 * rather than a change here.
 */
export interface Rule {
  readonly name: string;
  run(ctx: MarkContext): void;
}

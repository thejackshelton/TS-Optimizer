import type { Rule } from "./rule.js";
import { eventBoundaryRule } from "./event-boundaries/index.js";
import { componentRule } from "./components/index.js";

/** The built-in rules, run in order. Add a capability by adding a rule here. */
export const RULES: readonly Rule[] = [eventBoundaryRule, componentRule];

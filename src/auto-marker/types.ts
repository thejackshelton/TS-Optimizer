/** Where a marked or skipped closure lives, for the report. */
export interface Site {
  path: string;
  line: number;
  prop: string;
}

export interface MarkedDecision {
  kind: "marked";
  site: Site;
}

export interface SkippedDecision {
  kind: "skipped";
  site: Site;
  reason: string;
}

/** One decision per candidate closure, marked or skipped with a reason. */
export type Decision = MarkedDecision | SkippedDecision;

export interface CompiledFile {
  code: string;
  changed: boolean;
}

export interface CompileResult {
  files: Map<string, CompiledFile>;
  decisions: Decision[];
}

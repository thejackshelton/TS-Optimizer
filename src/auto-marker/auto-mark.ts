import { Analyzer } from "yuku-analyzer";
import { MarkContext } from "./core/context.js";
import { RULES } from "./rules/index.js";
import type { Decision } from "./types.js";
import type { TransformModuleInput } from "../optimizer/types/types.js";
import { mkSourceText } from "../optimizer/types/brands.js";

export interface AutoMarkResult {
  input: TransformModuleInput[];
  decisions: Decision[];
}

/**
 * Mark Qwik serialization boundaries across a whole input batch and hand the
 * marked source back in the optimizer's own input shape.
 */
export function autoMark(input: readonly TransformModuleInput[]): AutoMarkResult {
  const analyzer = new Analyzer();
  for (const file of input) analyzer.addFile(file.path, file.code);

  const ctx = new MarkContext(analyzer);
  for (const rule of RULES) rule.run(ctx);
  const { files, decisions } = ctx.build();

  const marked = input.map((file) => {
    const out = files.get(file.path);
    if (!out?.changed) return file;
    const { program: _, module: __, ...rest } = file;
    return { ...rest, code: mkSourceText(out.code) };
  });

  return { input: marked, decisions };
}

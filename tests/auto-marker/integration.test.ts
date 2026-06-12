import { expect, test } from "vitest";
import { autoMark } from "../../src/auto-marker/auto-mark.js";
import { mkFilePath, mkSourceText } from "../../src/optimizer/types/brands.js";

test("autoMark applies every rule and returns optimizer-shaped input", () => {
  const input = [
    {
      path: mkFilePath("app.tsx"),
      code: mkSourceText(`export function App() {
  return <button onClick={() => go()}>x</button>;
}
`),
    },
  ];

  const { input: out, decisions } = autoMark(input);
  const code = out[0]!.code;

  expect(code).toContain("component$(");
  expect(code).toContain("onClick$");
  expect(decisions.filter((d) => d.kind === "marked").length).toBeGreaterThan(0);
});

test("autoMark leaves a file with no boundaries unchanged", () => {
  const code = mkSourceText(`export const sum = (a, b) => a + b;\n`);
  const { input: out } = autoMark([{ path: mkFilePath("util.ts"), code }]);
  expect(out[0]!.code).toBe(code);
});

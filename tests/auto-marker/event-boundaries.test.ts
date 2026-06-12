import { expect, test } from "vitest";
import { createProject, report } from "./utils.js";

test("marks a direct host event handler", () => {
  const result = createProject({
    "app.tsx": `import { useSignal } from "@qwik.dev/core";
export function App() {
  const count = useSignal(0);
  return <button onClick={() => count.value++}>+1</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("marks across several host event types", () => {
  const result = createProject({
    "app.tsx": `export const App = () => (
  <form onSubmit={(e) => send(e)}>
    <input onChange={(e) => set(e)} />
    <button onKeyDown={(e) => key(e)}>go</button>
  </form>
);
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("ignores non-event host attributes", () => {
  const result = createProject({
    "app.tsx": `export const App = () => <div title={() => "x"}>hi</div>;`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("leaves already-marked handlers untouched", () => {
  const src = `export const App = () => <button onClick$={() => go()} />;`;
  const result = createProject({ "app.tsx": src });
  expect(result.marked.get("app.tsx")).toBe(src);
});

test("allows safe captures (module const, signal, import)", () => {
  const result = createProject({
    "consts.ts": `export const STEP = 2;`,
    "app.tsx": `import { useSignal } from "@qwik.dev/core";
import { STEP } from "./consts.ts";
export function App() {
  const count = useSignal(0);
  return <button onClick={() => (count.value += STEP)}>+</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("rejects a reassigned capture", () => {
  const result = createProject({
    "app.tsx": `export function App() {
  let n = 0;
  return <button onClick={() => n++}>x</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("rejects a local function capture", () => {
  const result = createProject({
    "app.tsx": `export function App() {
  function run() { go(); }
  return <button onClick={() => run()}>x</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("rejects a non-serializable capture", () => {
  const result = createProject({
    "app.tsx": `export function App() {
  const socket = new WebSocket("");
  return <button onClick={() => socket.send("x")}>x</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

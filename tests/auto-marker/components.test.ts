import { expect, test } from "vitest";
import { componentRule } from "../../src/auto-marker/rules/components/index.js";
import { createProject, report } from "./utils.js";

const RULES = [componentRule];

test("wraps an exported function component and adds the import", () => {
  const result = createProject(
    {
      "app.tsx": `export function App() {
  return <main>hi</main>;
}
`,
    },
    RULES,
  );
  expect(report(result)).toMatchSnapshot();
});

test("wraps a const arrow component", () => {
  const result = createProject({ "app.tsx": `export const App = () => <main>hi</main>;` }, RULES);
  expect(report(result)).toMatchSnapshot();
});

test("extends an existing core import instead of duplicating it", () => {
  const result = createProject(
    {
      "app.tsx": `import { useSignal } from "@qwik.dev/core";
export function App() {
  const n = useSignal(0);
  return <button>{n.value}</button>;
}
`,
    },
    RULES,
  );
  expect(report(result)).toMatchSnapshot();
});

test("wraps every component in a forwarding chain", () => {
  const result = createProject(
    {
      "button.tsx": `export function Button({ children }) { return <button>{children}</button>; }
`,
      "app.tsx": `import { Button } from "./button.tsx";
export function App() { return <Button>go</Button>; }
`,
    },
    RULES,
  );
  expect(report(result)).toMatchSnapshot();
});

test("does not wrap a helper that is called", () => {
  const result = createProject(
    {
      "app.tsx": `function row(x) { return <li>{x}</li>; }
export function App() { return <ul>{[1, 2].map(row)}</ul>; }
`,
    },
    RULES,
  );
  expect(report(result)).toMatchSnapshot();
});

test("does not wrap a non-JSX function", () => {
  const result = createProject({ "app.tsx": `export function add(a, b) { return a + b; }` }, RULES);
  expect(report(result)).toMatchSnapshot();
});

test("does not wrap an inline render-prop arrow", () => {
  const result = createProject(
    {
      "app.tsx": `export function List({ render }) { return <ul>{render()}</ul>; }
export function App() { return <List render={(x) => <li>{x}</li>} />; }
`,
    },
    RULES,
  );
  expect(report(result)).toMatchSnapshot();
});

test("leaves an already-wrapped component untouched", () => {
  const src = `import { component$ } from "@qwik.dev/core";
export const App = component$(() => <main>hi</main>);
`;
  const result = createProject({ "app.tsx": src }, RULES);
  expect(result.decisions).toHaveLength(0);
  expect(result.marked.get("app.tsx")).toBe(src);
});

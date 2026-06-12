import { expect, test } from "vitest";
import { createProject, recompile, report } from "./utils.js";

test("one hop through a destructured prop", () => {
  const result = createProject({
    "app.tsx": `import { Button } from "./button.tsx";
export const App = () => <Button onPress={() => save()} />;
`,
    "button.tsx": `export function Button({ onPress }) {
  return <button onClick={onPress}>Go</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("through a props-object member access", () => {
  const result = createProject({
    "app.tsx": `import { Button } from "./button.tsx";
export const App = () => <Button onPress={() => save()} />;
`,
    "button.tsx": `export function Button(props) {
  return <button onClick={props.onPress}>Go</button>;
}
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("through a renamed destructuring", () => {
  const result = createProject({
    "button.tsx": `export function Button({ onPress: handlePress }) {
  return <button onClick={handlePress}>Go</button>;
}
`,
    "app.tsx": `import { Button } from "./button.tsx";
export const App = () => <Button onPress={() => save()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("through a same-file wrapper", () => {
  const result = createProject({
    "app.tsx": `function Wrapper({ onTrigger }) {
  return <button onClick={onTrigger}>Go</button>;
}
export const App = () => <Wrapper onTrigger={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("two hops", () => {
  const result = createProject({
    "app.tsx": `function Inner({ onTap }) { return <button onClick={onTap} />; }
function Outer({ onRun }) { return <Inner onTap={onRun} />; }
export const App = () => <Outer onRun={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("three hops", () => {
  const result = createProject({
    "app.tsx": `function A({ onTap }) { return <button onClick={onTap} />; }
function B({ onRun }) { return <A onTap={onRun} />; }
function C({ onGo }) { return <B onRun={onGo} />; }
export const App = () => <C onGo={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("multiple host events on one component", () => {
  const result = createProject({
    "app.tsx": `function Surface({ onEnter, onLeave }) {
  return <div onMouseEnter={onEnter} onMouseLeave={onLeave} />;
}
export const App = () => <Surface onEnter={() => a()} onLeave={() => b()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("two independent boundaries on one component", () => {
  const result = createProject({
    "app.tsx": `function Actions({ onConfirm, onCancel }) {
  return (
    <div>
      <button onClick={onCancel}>cancel</button>
      <button onClick={onConfirm}>ok</button>
    </div>
  );
}
export const App = () => <Actions onConfirm={() => a()} onCancel={() => b()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("forwarding into a row component", () => {
  const result = createProject({
    "app.tsx": `function Row({ onSelect }) { return <button onClick={onSelect} />; }
function List({ onChoose }) { return <Row onSelect={onChoose} />; }
export const App = () => <List onChoose={() => pick()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("resolves an aliased import", () => {
  const result = createProject({
    "button.tsx": `export function Button({ onPress }) {
  return <button onClick={onPress} />;
}
`,
    "app.tsx": `import { Button as Action } from "./button.tsx";
export const App = () => <Action onPress={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("marks some props and skips others on one component", () => {
  const result = createProject({
    "app.tsx": `function Btn({ onPress }) { return <button onClick={onPress} />; }
function Panel({ onSave, onPreview }) {
  return (
    <div>
      <Btn onPress={onSave} />
      <p>{onPreview()}</p>
    </div>
  );
}
export const App = () => <Panel onSave={() => s()} onPreview={() => "x"} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("skips a render callback", () => {
  const result = createProject({
    "app.tsx": `function Renderer({ render }) { return <div>{render()}</div>; }
export const App = () => <Renderer render={() => "x"} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("skips conditional forwarding", () => {
  const result = createProject({
    "app.tsx": `function Btn({ enabled, onTap }) {
  return <button onClick={enabled ? onTap : undefined} />;
}
export const App = () => <Btn enabled={true} onTap={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("skips a prop passed to a helper call", () => {
  const result = createProject({
    "app.tsx": `function Panel({ onReady }) {
  const handle = register(onReady);
  return <div>{String(handle)}</div>;
}
export const App = () => <Panel onReady={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("skips spread props", () => {
  const result = createProject({
    "app.tsx": `function Btn(props) { return <button onClick={props.onPress} />; }
export const App = () => <Btn {...rest} onPress={() => go()} />;
`,
  });
  expect(report(result)).toMatchSnapshot();
});

test("output is idempotent", () => {
  const once = createProject({
    "app.tsx": `function Inner({ onTap }) { return <button onClick={onTap} />; }
function Outer({ onRun }) { return <Inner onTap={onRun} />; }
export const App = () => <Outer onRun={() => go()} />;
`,
  });
  const twice = recompile(once);
  for (const [path, code] of twice.marked) expect(code).toBe(once.marked.get(path));
});

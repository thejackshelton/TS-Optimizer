// Markerless -> optimized, end to end.
//
// Feeds a small Qwik project that contains ZERO `$` markers through the
// auto-marker, then through the optimizer, and prints each stage so you can
// see the whole thing by eye. Run with: pnpm demo:automark
import { transformModule } from "../dist/index.js";
import { autoMark } from "../dist/auto-marker/auto-mark.js";

const files = {
  "app.tsx": `import { useSignal } from "@qwik.dev/core";
import { Toolbar } from "./components/toolbar.tsx";
import { List } from "./components/list.tsx";

export function App() {
  const count = useSignal(0);
  const saved = useSignal(false);
  return (
    <main>
      <button onClick={() => count.value++}>count is {count.value}</button>
      <Toolbar onSave={() => { saved.value = true; }} onPublish={() => count.value++} />
      <List items={["a", "b", "c"]} renderItem={(item) => <span>{item}</span>} />
    </main>
  );
}
`,
  "components/toolbar.tsx": `import { ToolbarButton } from "./toolbar-button.tsx";

export function Toolbar({ onSave, onPublish }) {
  return (
    <div>
      <ToolbarButton label="Save" onActivate={onSave} />
      <ToolbarButton label="Publish" onActivate={onPublish} />
    </div>
  );
}
`,
  "components/toolbar-button.tsx": `import { Button } from "./button.tsx";

export function ToolbarButton(props) {
  return <Button onPress={props.onActivate}>{props.label}</Button>;
}
`,
  "components/button.tsx": `export function Button({ onPress, children }) {
  return <button type="button" onClick={onPress}>{children}</button>;
}
`,
  "components/list.tsx": `export function List({ items, renderItem }) {
  return <ul>{items.map((item) => <li>{renderItem(item)}</li>)}</ul>;
}
`,
};

const banner = (label) => console.log(`\n${"=".repeat(70)}\n  ${label}\n${"=".repeat(70)}`);

const input = Object.entries(files).map(([path, code]) => ({ path, code }));

banner("1. WHAT YOU WROTE  (no $ markers anywhere)");
for (const [path, code] of Object.entries(files)) console.log(`\n--- ${path} ---\n${code.trimEnd()}`);

const { input: marked, decisions } = autoMark(input);

banner("2. AUTO-MARK DECISIONS");
for (const d of decisions) {
  console.log(
    d.kind === "marked"
      ? `  mark  ${d.site.path}:${d.site.line} ${d.site.prop}`
      : `  skip  ${d.site.path}:${d.site.line} ${d.site.prop}  (${d.reason})`,
  );
}

banner("3. OPTIMIZER OUTPUT  (markerless source -> QRL segments)");
const out = transformModule({ input: marked, srcDir: ".", entryStrategy: { type: "segment" } });
for (const m of out.modules) console.log(`\n--- [${m.kind}] ${m.path} ---\n${m.code.trimEnd()}`);

if (out.diagnostics.length) {
  banner("DIAGNOSTICS");
  for (const d of out.diagnostics) console.log(`  ${d.message}`);
}

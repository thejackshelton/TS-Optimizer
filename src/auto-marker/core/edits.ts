export interface Edit {
  start: number;
  end: number;
  text: string;
}

/** Apply non-overlapping span edits to a source string. */
export function applyEdits(source: string, edits: Edit[]): string {
  const sorted = [...edits].sort((a, b) => a.start - b.start || a.end - b.end);
  let out = "";
  let cursor = 0;
  for (const edit of sorted) {
    if (edit.start < cursor) throw new Error(`overlapping edits at offset ${edit.start}`);
    out += source.slice(cursor, edit.start) + edit.text;
    cursor = edit.end;
  }
  return out + source.slice(cursor);
}

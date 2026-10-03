// Reads CHANGELOG.md (and a release's notes, which are one of its sections) for display.
// Only what the file uses is understood: `## [version] — date`, `### group`, `- item`.

export interface ChangeGroup {
  title: string;
  items: string[];
}

export interface ChangeEntry {
  /** `a.b.cdd`, or null for the "not released" section. */
  version: string | null;
  date: string | null;
  groups: ChangeGroup[];
}

const HEADING = /^##\s+\[([^\]]+)\]\s*(?:[—–-]\s*(\S+))?\s*$/;

/** The `###` groups with their `- ` items from lines of one section. */
export function parseGroups(lines: string[]): ChangeGroup[] {
  const groups: ChangeGroup[] = [];
  for (const raw of lines) {
    const line = raw.trimEnd();
    const title = /^###\s+(.+)$/.exec(line);
    if (title) {
      groups.push({ title: title[1].trim(), items: [] });
    } else if (/^\s*[-*]\s+/.test(line)) {
      // Items before any `###` belong to a group without a title.
      if (groups.length === 0) groups.push({ title: "", items: [] });
      groups[groups.length - 1].items.push(line.replace(/^\s*[-*]\s+/, ""));
    } else if (/^\s{2,}\S/.test(line) && groups.at(-1)?.items.length) {
      // A wrapped continuation of the previous item.
      const items = groups[groups.length - 1].items;
      items[items.length - 1] += " " + line.trim();
    }
  }
  return groups.filter((g) => g.items.length > 0);
}

/** Every entry of the changelog, newest first as written. */
export function parseChangelog(text: string): ChangeEntry[] {
  const entries: ChangeEntry[] = [];
  let lines: string[] = [];
  let head: { version: string | null; date: string | null } | null = null;
  const flush = () => {
    if (head) entries.push({ ...head, groups: parseGroups(lines) });
    lines = [];
  };
  for (const line of text.split(/\r?\n/)) {
    const m = HEADING.exec(line);
    if (m) {
      flush();
      const isVersion = /^\d+\.\d+\.\d{3}$/.test(m[1]);
      head = { version: isVersion ? m[1] : null, date: m[2] ?? null };
    } else if (head) {
      lines.push(line);
    }
  }
  flush();
  // The "not released" section is shown only when it has something in it.
  return entries.filter((e) => e.version !== null || e.groups.length > 0);
}

/** An item split into plain and `code` pieces (backticks), for showing without raw markup. */
export function inline(text: string): { text: string; code: boolean }[] {
  // Links lose their address: `[text](url)` shows as `text`.
  const plain = text.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");
  return plain
    .split(/(`[^`]+`)/)
    .filter((p) => p !== "")
    .map((p) =>
      p.startsWith("`") && p.endsWith("`") && p.length > 2
        ? { text: p.slice(1, -1), code: true }
        : { text: p, code: false },
    );
}

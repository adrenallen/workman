export interface ScratchpadOutlineItem {
  id: string;
  level: 2 | 3;
  line: number;
  label: string;
}

function plainHeading(markdown: string): string {
  return markdown
    .replace(/!\[([^\]]*)\]\([^)]+\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[*_~`]/g, '')
    .trim();
}

export function scratchpadOutline(markdown: string): ScratchpadOutlineItem[] {
  const items: ScratchpadOutlineItem[] = [];
  let fenced = false;
  const lines = markdown.replaceAll('\r\n', '\n').split('\n');

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\s*```/.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) continue;
    const heading = /^(#{2,3})\s+(.+?)\s*#*\s*$/.exec(line);
    if (!heading) continue;
    const label = plainHeading(heading[2]);
    if (!label) continue;
    const lineNumber = index + 1;
    items.push({
      id: `scratchpad-heading-${lineNumber}`,
      level: heading[1].length as 2 | 3,
      line: lineNumber,
      label
    });
  }

  return items;
}

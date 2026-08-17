/** Parse a shell-like argument field into literal argv entries for safe server-side quoting. */
export function parseExtraArgs(source: string): string[] {
  const args: string[] = [];
  let current = '';
  let quote: 'single' | 'double' | null = null;
  let escaped = false;
  let started = false;
  for (const character of source) {
    if (escaped) {
      current += quote === 'double' ? decodeQuotedEscape(character) : character;
      escaped = false;
      started = true;
    } else if (character === '\\' && quote !== 'single') {
      escaped = true;
      started = true;
    } else if (character === "'" && quote !== 'double') {
      quote = quote === 'single' ? null : 'single';
      started = true;
    } else if (character === '"' && quote !== 'single') {
      quote = quote === 'double' ? null : 'double';
      started = true;
    } else if (/\s/.test(character) && quote === null) {
      if (started) args.push(current);
      current = '';
      started = false;
    } else {
      current += character;
      started = true;
    }
  }
  if (escaped) current += '\\';
  if (quote !== null) throw new Error('Close the quoted launch argument before spawning');
  if (started) args.push(current);
  return args;
}

function decodeQuotedEscape(character: string): string {
  switch (character) {
    case 'n':
      return '\n';
    case 'r':
      return '\r';
    case 't':
      return '\t';
    case 'b':
      return '\b';
    case 'f':
      return '\f';
    default:
      return character;
  }
}

/** Format literal argv entries for a shell-like field parsed by `parseExtraArgs`. */
export function formatExtraArgs(args: string[]): string {
  return args
    .map((arg) => (/^[\w./:@%+=,-]+$/.test(arg) ? arg : JSON.stringify(arg)))
    .join(' ');
}

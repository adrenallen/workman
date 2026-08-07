const safePathCharacter = /^[\p{L}\p{N}_./-]$/u;

/** Match Terminal.app's unquoted, backslash-escaped file-path insertion. */
export function shellEscapePath(path: string): string {
  if (!path) throw new Error('Dropped file path is empty.');
  if (/[\0\r\n]/u.test(path)) {
    throw new Error('Dropped file paths cannot contain control characters.');
  }
  return Array.from(path, (character) =>
    safePathCharacter.test(character) ? character : `\\${character}`
  ).join('');
}

export function shellEscapePaths(paths: string[]): string {
  if (paths.length === 0) throw new Error('No dropped file paths were provided.');
  return paths.map(shellEscapePath).join(' ');
}

export function pointIsInsideRect(
  point: { x: number; y: number },
  rect: { left: number; top: number; right: number; bottom: number },
  physicalScale: number
): boolean {
  if (!Number.isFinite(physicalScale) || physicalScale <= 0) return false;
  const x = point.x / physicalScale;
  const y = point.y / physicalScale;
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

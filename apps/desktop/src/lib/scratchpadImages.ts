export interface ScratchpadMarkdownImage {
  from: number;
  to: number;
  alt: string;
  source: string;
}

/** Find the simple inline image form emitted by Workman's recorded-feedback packets. */
export function scratchpadMarkdownImages(text: string): ScratchpadMarkdownImage[] {
  const images: ScratchpadMarkdownImage[] = [];
  for (const match of text.matchAll(/!\[([^\]\n]*)\]\((?:<([^>\n]+)>|([^\)\n]+))\)/g)) {
    const from = match.index ?? 0;
    images.push({
      from,
      to: from + match[0].length,
      alt: match[1],
      source: match[2] ?? match[3]
    });
  }
  return images;
}

/** Only local absolute paths are loaded through Workman's bounded native image reader. */
export function scratchpadLocalImagePath(source: string): string | null {
  const value = source.trim();
  if (!value.startsWith('/') && !/^[A-Za-z]:[\\/]/.test(value)) return null;
  try {
    return decodeURIComponent(value);
  } catch {
    return null;
  }
}

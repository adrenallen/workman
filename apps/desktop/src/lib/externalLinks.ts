import { isBrowserUrl, openBrowserUrl } from './openers';

export const EXTERNAL_LINK_TOOLTIP = 'Cmd+click to open in browser';

export function openExternalUrl(
  url: string,
  onError: (message: string) => void = (message) => console.error(message)
): void {
  if (!isBrowserUrl(url)) {
    onError('Browser links must use http or https.');
    return;
  }
  void openBrowserUrl(url).catch((cause) => {
    onError(cause instanceof Error ? cause.message : String(cause));
  });
}

export function markdownLinkAt(source: string, offset: number): string | null {
  const pattern = /\[[^\]\n]+\]\(([^)\n]+)\)/g;
  for (const match of source.matchAll(pattern)) {
    const from = match.index ?? 0;
    const to = from + match[0].length;
    if (offset >= from && offset <= to && isBrowserUrl(match[1])) return match[1];
  }
  return null;
}

export function installExternalLinkGuard(root: Document = document): () => void {
  const onClick = (event: MouseEvent) => {
    if (event.defaultPrevented || event.button !== 0) return;
    const target = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (!(target instanceof HTMLAnchorElement)) return;

    const href = target.getAttribute('href') ?? '';
    if (!href || href.startsWith('#')) return;
    if (isBrowserUrl(href)) {
      event.preventDefault();
      openExternalUrl(href);
      return;
    }

    // Absolute non-web schemes are never allowed to reach the embedded webview.
    if (/^[a-z][a-z0-9+.-]*:/i.test(href)) event.preventDefault();
  };
  root.addEventListener('click', onClick, true);
  return () => root.removeEventListener('click', onClick, true);
}

interface DocumentPosition {
  top?: number;
  left?: number;
  anchor?: number;
  head?: number;
}

const positions = new Map<string, DocumentPosition>();

export function documentPosition(key: string): DocumentPosition {
  return positions.get(key) ?? {};
}

export function rememberDocumentPosition(key: string, position: DocumentPosition): void {
  const next = { ...positions.get(key), ...position };
  positions.delete(key);
  positions.set(key, next);
  if (positions.size > 100) positions.delete(positions.keys().next().value!);
}

/** Restore the outer document scroller after CodeMirror has measured its initial viewport. */
export function rememberDocumentScroll(node: HTMLElement, key: string | undefined) {
  let frame = 0;
  let restoring = false;
  let last = { top: node.scrollTop, left: node.scrollLeft };
  function save(): void {
    if (!key || restoring) return;
    last = { top: node.scrollTop, left: node.scrollLeft };
    rememberDocumentPosition(key, last);
  }
  function restore(): void {
    if (!key) return;
    const saved = documentPosition(key);
    restoring = true;
    let remaining = 3;
    const apply = () => {
      node.scrollTo({ top: saved.top ?? 0, left: saved.left ?? 0, behavior: 'instant' });
      if (--remaining > 0) frame = requestAnimationFrame(apply);
      else { restoring = false; save(); }
    };
    frame = requestAnimationFrame(apply);
  }
  function stopRestoring(): void {
    cancelAnimationFrame(frame);
    restoring = false;
  }
  node.addEventListener('scroll', save, { passive: true });
  node.addEventListener('wheel', stopRestoring, { passive: true });
  node.addEventListener('pointerdown', stopRestoring);
  restore();
  return {
    update(next: string | undefined) {
      if (next === key) return;
      if (key) rememberDocumentPosition(key, last);
      stopRestoring();
      key = next;
      restore();
    },
    destroy() {
      stopRestoring();
      if (key) rememberDocumentPosition(key, last);
      node.removeEventListener('scroll', save);
      node.removeEventListener('wheel', stopRestoring);
      node.removeEventListener('pointerdown', stopRestoring);
    }
  };
}

/** Preserve selection mapping when an agent changes only part of the document. */
export function markdownChange(previous: string, next: string) {
  let from = 0;
  while (from < previous.length && from < next.length && previous[from] === next[from]) from++;
  let to = previous.length;
  let end = next.length;
  while (to > from && end > from && previous[to - 1] === next[end - 1]) { to--; end--; }
  return { from, to, insert: next.slice(from, end) };
}

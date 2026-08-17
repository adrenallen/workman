function numericPixels(value: string): number | null {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function resizeTextarea(node: HTMLTextAreaElement, minimumHeight: number): void {
  node.style.height = 'auto';
  const maximumHeight = numericPixels(getComputedStyle(node).maxHeight);
  const contentHeight = Math.max(minimumHeight, node.scrollHeight);
  const height = maximumHeight === null ? contentHeight : Math.min(contentHeight, maximumHeight);
  node.style.height = `${height}px`;
  node.style.overflowY = maximumHeight !== null && contentHeight > maximumHeight ? 'auto' : 'hidden';
}

/**
 * Workman multiline convention: Enter submits the containing form, while
 * Shift+Enter keeps the textarea's native newline behavior. The textarea grows
 * with its content until its CSS max-height, preserving modal scroll frames.
 */
export function submitOnEnter(node: HTMLTextAreaElement): { destroy: () => void } {
  const minimumHeight = node.getBoundingClientRect().height;
  const form = node.form;
  let resizeFrame: number | null = null;
  const resize = () => resizeTextarea(node, minimumHeight);
  const scheduleResize = () => {
    if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = null;
      resize();
    });
  };
  const handleKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    const submitter = Array.from(form?.querySelectorAll<HTMLButtonElement | HTMLInputElement>(
      'button[type="submit"]:not(:disabled), input[type="submit"]:not(:disabled)'
    ) ?? []).find((candidate) => candidate.form === form);
    if (form && submitter) form.requestSubmit(submitter);
    scheduleResize();
  };

  node.addEventListener('input', resize);
  node.addEventListener('keydown', handleKeydown);
  form?.addEventListener('submit', scheduleResize);
  scheduleResize();

  return {
    destroy() {
      node.removeEventListener('input', resize);
      node.removeEventListener('keydown', handleKeydown);
      form?.removeEventListener('submit', scheduleResize);
      if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
    }
  };
}

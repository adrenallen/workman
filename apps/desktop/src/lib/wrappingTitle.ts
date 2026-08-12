export function singleLineTitle(value: string): string {
  return value.replace(/[\r\n]+/g, ' ');
}

export function autoGrowTextarea(node: HTMLTextAreaElement, _value: string) {
  let frame: number | null = null;
  let observedWidth = -1;

  function fit(): void {
    frame = null;
    node.style.height = 'auto';
    node.style.height = `${node.scrollHeight}px`;
  }

  function scheduleFit(): void {
    if (frame !== null) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(fit);
  }

  const resizeObserver = new ResizeObserver(([entry]) => {
    const width = entry?.contentRect.width ?? node.clientWidth;
    if (width === observedWidth) return;
    observedWidth = width;
    scheduleFit();
  });

  resizeObserver.observe(node);
  scheduleFit();

  return {
    update(_nextValue: string): void {
      scheduleFit();
    },
    destroy(): void {
      resizeObserver.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    }
  };
}

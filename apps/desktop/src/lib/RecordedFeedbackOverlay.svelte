<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { emit, listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';

  type Tool = 'pointer' | 'pen' | 'arrow' | 'rectangle' | 'ellipse';
  type Point = { x: number; y: number };
  type Stroke = { display_index: number; tool: Tool; color: string; width: number; points: Point[] };

  const displayIndex = Number(getCurrentWindow().label.replace('feedback-overlay-', '')) || 0;
  let canvas: HTMLCanvasElement;
  let context: CanvasRenderingContext2D | null = null;
  let tool = $state<Tool>('pointer');
  let color = $state('#ff4d5e');
  let width = $state(4);
  let strokes = $state<Stroke[]>([]);
  let draft = $state<Stroke | null>(null);
  let selecting = $state(false);
  let selectionStart = $state<Point | null>(null);
  let selectionEnd = $state<Point | null>(null);

  onMount(() => {
    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);
    window.addEventListener('keydown', handleKeydown);
    const unlisteners = Promise.all([
      listen<{ tool: Tool; color: string; width: number }>('feedback://tool', (event) => {
        tool = event.payload.tool;
        color = event.payload.color;
        width = event.payload.width;
      }),
      listen<{ selecting: boolean }>('feedback://region', (event) => {
        selecting = event.payload.selecting;
        selectionStart = null;
        selectionEnd = null;
        paint();
      }),
      listen<Stroke[]>('feedback://annotations', (event) => {
        strokes = event.payload.filter((stroke) => stroke.display_index === displayIndex);
        paint();
      })
    ]);
    return () => {
      window.removeEventListener('resize', resizeCanvas);
      window.removeEventListener('keydown', handleKeydown);
      void unlisteners.then((values) => values.forEach((unlisten) => unlisten()));
    };
  });

  function resizeCanvas(): void {
    const scale = window.devicePixelRatio || 1;
    canvas.width = Math.round(window.innerWidth * scale);
    canvas.height = Math.round(window.innerHeight * scale);
    canvas.style.width = `${window.innerWidth}px`;
    canvas.style.height = `${window.innerHeight}px`;
    context = canvas.getContext('2d');
    context?.setTransform(scale, 0, 0, scale, 0, 0);
    paint();
  }

  function pointerDown(event: PointerEvent): void {
    if (!selecting && tool === 'pointer') return;
    canvas.setPointerCapture(event.pointerId);
    const point = localPoint(event);
    if (selecting) {
      selectionStart = point;
      selectionEnd = point;
    } else {
      draft = { display_index: displayIndex, tool, color, width, points: [point, point] };
    }
    paint();
  }

  function pointerMove(event: PointerEvent): void {
    if (selecting && selectionStart) selectionEnd = localPoint(event);
    else if (draft) {
      const point = localPoint(event);
      if (draft.tool === 'pen') draft = { ...draft, points: [...draft.points, point] };
      else draft = { ...draft, points: [draft.points[0], point] };
    }
    paint();
  }

  async function pointerUp(event: PointerEvent): Promise<void> {
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
    if (selecting && selectionStart && selectionEnd) {
      const region = normalizedRegion(selectionStart, selectionEnd);
      selectionStart = null;
      selectionEnd = null;
      if (region.width < 4 || region.height < 4) {
        await cancelRegion();
        return;
      }
      try {
        await invoke('feedback_capture_snapshot', { displayIndex, region });
        selecting = false;
      } catch (cause) {
        await emit('feedback://ui-error', { message: messageFor(cause) });
      }
    } else if (draft) {
      const finished = draft;
      draft = null;
      strokes = [...strokes, finished];
      try { await invoke('feedback_record_stroke', { stroke: finished }); }
      catch (cause) {
        strokes = strokes.slice(0, -1);
        await emit('feedback://ui-error', { message: messageFor(cause) });
      }
    }
    paint();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && selecting) {
      event.preventDefault();
      void cancelRegion();
    }
  }

  async function cancelRegion(): Promise<void> {
    selecting = false;
    selectionStart = null;
    selectionEnd = null;
    paint();
    try { await invoke('feedback_cancel_region'); }
    catch (cause) { await emit('feedback://ui-error', { message: messageFor(cause) }); }
  }

  function paint(): void {
    if (!context || !canvas) return;
    context.clearRect(0, 0, window.innerWidth, window.innerHeight);
    if (selecting) {
      context.fillStyle = 'rgb(4 6 10 / 0.16)';
      context.fillRect(0, 0, window.innerWidth, window.innerHeight);
    }
    for (const stroke of strokes) drawStroke(stroke);
    if (draft) drawStroke(draft);
    if (selecting && selectionStart && selectionEnd) drawSelection(selectionStart, selectionEnd);
  }

  function drawStroke(stroke: Stroke): void {
    if (!context || stroke.points.length < 2) return;
    const [start] = stroke.points;
    const end = stroke.points[stroke.points.length - 1];
    context.save();
    context.lineCap = 'round';
    context.lineJoin = 'round';
    context.strokeStyle = stroke.color;
    context.lineWidth = stroke.width;
    context.shadowColor = 'rgb(0 0 0 / 0.85)';
    context.shadowBlur = 2;
    context.beginPath();
    if (stroke.tool === 'pen') {
      context.moveTo(start.x, start.y);
      for (const point of stroke.points.slice(1)) context.lineTo(point.x, point.y);
    } else if (stroke.tool === 'rectangle') {
      context.rect(start.x, start.y, end.x - start.x, end.y - start.y);
    } else if (stroke.tool === 'ellipse') {
      context.ellipse((start.x + end.x) / 2, (start.y + end.y) / 2,
        Math.max(1, Math.abs(end.x - start.x) / 2), Math.max(1, Math.abs(end.y - start.y) / 2), 0, 0, Math.PI * 2);
    } else {
      context.moveTo(start.x, start.y);
      context.lineTo(end.x, end.y);
    }
    context.stroke();
    if (stroke.tool === 'arrow') drawArrowHead(start, end, stroke.width);
    context.restore();
  }

  function drawArrowHead(start: Point, end: Point, strokeWidth: number): void {
    if (!context) return;
    const angle = Math.atan2(end.y - start.y, end.x - start.x);
    const length = Math.max(12, strokeWidth * 5);
    context.beginPath();
    context.moveTo(end.x, end.y);
    context.lineTo(end.x - length * Math.cos(angle - .65), end.y - length * Math.sin(angle - .65));
    context.moveTo(end.x, end.y);
    context.lineTo(end.x - length * Math.cos(angle + .65), end.y - length * Math.sin(angle + .65));
    context.stroke();
  }

  function drawSelection(start: Point, end: Point): void {
    if (!context) return;
    const region = normalizedRegion(start, end);
    context.clearRect(region.x, region.y, region.width, region.height);
    context.save();
    context.strokeStyle = '#ffffff';
    context.lineWidth = 1;
    context.setLineDash([6, 4]);
    context.shadowColor = '#000000';
    context.shadowBlur = 3;
    context.strokeRect(region.x + .5, region.y + .5, region.width - 1, region.height - 1);
    context.restore();
    const label = `${Math.round(region.width)} × ${Math.round(region.height)}`;
    context.fillStyle = 'rgb(10 12 16 / 0.9)';
    context.fillRect(region.x, Math.max(0, region.y - 23), context.measureText(label).width + 14, 20);
    context.fillStyle = '#ffffff';
    context.font = '11px JetBrains Mono, monospace';
    context.fillText(label, region.x + 7, Math.max(14, region.y - 9));
  }

  function localPoint(event: PointerEvent): Point {
    const rect = canvas.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function normalizedRegion(start: Point, end: Point) {
    return {
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      width: Math.abs(end.x - start.x),
      height: Math.abs(end.y - start.y)
    };
  }

  function messageFor(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<canvas
  bind:this={canvas}
  aria-label={selecting ? 'Drag to select a screenshot region' : 'Feedback annotation surface'}
  class:selecting
  class:drawing={!selecting && tool !== 'pointer'}
  onpointerdown={pointerDown}
  onpointermove={pointerMove}
  onpointerup={(event) => void pointerUp(event)}
  onpointercancel={(event) => void pointerUp(event)}
></canvas>

<style>
  :global(html), :global(body), :global(#app) { width: 100%; height: 100%; margin: 0; overflow: hidden; background: transparent !important; }
  canvas { display: block; width: 100%; height: 100%; touch-action: none; }
  canvas.drawing { cursor: crosshair; }
  canvas.selecting { cursor: cell; }
</style>

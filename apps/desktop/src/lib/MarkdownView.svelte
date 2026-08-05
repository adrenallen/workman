<script lang="ts">
  interface Props {
    source: string;
  }

  type Block =
    | { kind: 'heading'; level: number; text: string }
    | { kind: 'paragraph'; text: string }
    | { kind: 'list'; ordered: boolean; items: string[] }
    | { kind: 'quote'; text: string }
    | { kind: 'code'; language: string; text: string }
    | { kind: 'rule' };

  type Inline =
    | { kind: 'text'; text: string }
    | { kind: 'strong'; text: string }
    | { kind: 'code'; text: string }
    | { kind: 'link'; text: string; href: string };

  let { source }: Props = $props();
  let blocks = $derived(parseMarkdown(source));

  function parseMarkdown(markdown: string): Block[] {
    const lines = markdown.replaceAll('\r\n', '\n').split('\n');
    const output: Block[] = [];
    let index = 0;

    while (index < lines.length) {
      const line = lines[index];
      if (!line.trim()) {
        index += 1;
        continue;
      }
      if (/^\s*```/.test(line)) {
        const language = line.replace(/^\s*```/, '').trim();
        const code: string[] = [];
        index += 1;
        while (index < lines.length && !/^\s*```/.test(lines[index])) {
          code.push(lines[index]);
          index += 1;
        }
        if (index < lines.length) index += 1;
        output.push({ kind: 'code', language, text: code.join('\n') });
        continue;
      }
      const heading = /^(#{1,6})\s+(.+)$/.exec(line);
      if (heading) {
        output.push({ kind: 'heading', level: heading[1].length, text: heading[2].trim() });
        index += 1;
        continue;
      }
      if (/^\s*(---+|___+|\*\*\*+)\s*$/.test(line)) {
        output.push({ kind: 'rule' });
        index += 1;
        continue;
      }
      if (/^\s*>/.test(line)) {
        const quote: string[] = [];
        while (index < lines.length && /^\s*>/.test(lines[index])) {
          quote.push(lines[index].replace(/^\s*>\s?/, ''));
          index += 1;
        }
        output.push({ kind: 'quote', text: quote.join(' ') });
        continue;
      }
      const list = /^\s*(?:([-+*])|(\d+)\.)\s+(.+)$/.exec(line);
      if (list) {
        const ordered = Boolean(list[2]);
        const items: string[] = [];
        while (index < lines.length) {
          const item = /^\s*(?:([-+*])|(\d+)\.)\s+(.+)$/.exec(lines[index]);
          if (!item || Boolean(item[2]) !== ordered) break;
          items.push(item[3]);
          index += 1;
        }
        output.push({ kind: 'list', ordered, items });
        continue;
      }

      const paragraph: string[] = [line.trim()];
      index += 1;
      while (
        index < lines.length &&
        lines[index].trim() &&
        !/^(#{1,6})\s+/.test(lines[index]) &&
        !/^\s*(?:```|>|[-+*]\s+|\d+\.\s+)/.test(lines[index]) &&
        !/^\s*(---+|___+|\*\*\*+)\s*$/.test(lines[index])
      ) {
        paragraph.push(lines[index].trim());
        index += 1;
      }
      output.push({ kind: 'paragraph', text: paragraph.join(' ') });
    }
    return output;
  }

  function inline(text: string): Inline[] {
    const tokens: Inline[] = [];
    const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\))/g;
    let offset = 0;
    for (const match of text.matchAll(pattern)) {
      if (match.index > offset) tokens.push({ kind: 'text', text: text.slice(offset, match.index) });
      const token = match[0];
      if (token.startsWith('`')) {
        tokens.push({ kind: 'code', text: token.slice(1, -1) });
      } else if (token.startsWith('**')) {
        tokens.push({ kind: 'strong', text: token.slice(2, -2) });
      } else {
        const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
        if (link && /^(https?:|mailto:)/i.test(link[2])) {
          tokens.push({ kind: 'link', text: link[1], href: link[2] });
        } else {
          tokens.push({ kind: 'text', text: token });
        }
      }
      offset = (match.index ?? 0) + token.length;
    }
    if (offset < text.length) tokens.push({ kind: 'text', text: text.slice(offset) });
    return tokens;
  }
</script>

{#snippet inlineContent(text: string)}
  {#each inline(text) as token}
    {#if token.kind === 'strong'}
      <strong>{token.text}</strong>
    {:else if token.kind === 'code'}
      <code>{token.text}</code>
    {:else if token.kind === 'link'}
      <a href={token.href} target="_blank" rel="noreferrer">{token.text}</a>
    {:else}
      {token.text}
    {/if}
  {/each}
{/snippet}

<div class="markdown">
  {#each blocks as block}
    {#if block.kind === 'heading'}
      {#if block.level === 1}<h1>{@render inlineContent(block.text)}</h1>
      {:else if block.level === 2}<h2>{@render inlineContent(block.text)}</h2>
      {:else if block.level === 3}<h3>{@render inlineContent(block.text)}</h3>
      {:else}<h4>{@render inlineContent(block.text)}</h4>{/if}
    {:else if block.kind === 'paragraph'}
      <p>{@render inlineContent(block.text)}</p>
    {:else if block.kind === 'quote'}
      <blockquote>{@render inlineContent(block.text)}</blockquote>
    {:else if block.kind === 'code'}
      <pre data-language={block.language || undefined}><code>{block.text}</code></pre>
    {:else if block.kind === 'list'}
      {#if block.ordered}
        <ol>{#each block.items as item}<li>{@render inlineContent(item)}</li>{/each}</ol>
      {:else}
        <ul>{#each block.items as item}<li>{@render inlineContent(item)}</li>{/each}</ul>
      {/if}
    {:else}
      <hr />
    {/if}
  {/each}
</div>

<style>
  .markdown {
    color: var(--muted-foreground);
    font-size: var(--font-size-sm);
    line-height: 1.65;
  }

  .markdown :global(h1),
  .markdown :global(h2),
  .markdown :global(h3),
  .markdown :global(h4) {
    margin: var(--space-4) 0 var(--space-2);
    color: var(--foreground);
    font-family: var(--ui-font-family);
    line-height: 1.15;
  }

  .markdown :global(h1:first-child),
  .markdown :global(h2:first-child),
  .markdown :global(h3:first-child) {
    margin-top: 0;
  }

  .markdown :global(h1) { font-size: var(--font-size-xl); }
  .markdown :global(h2) { font-size: var(--font-size-lg); }
  .markdown :global(h3) { font-size: var(--font-size-base); }
  .markdown :global(h4) { font-size: var(--font-size-xs); text-transform: uppercase; letter-spacing: 0.05em; }

  .markdown :global(p) { margin: 0 0 var(--space-3); }
  .markdown :global(ul), .markdown :global(ol) { margin: 0 0 var(--space-4); padding-left: var(--space-4); }
  .markdown :global(li) { margin: var(--space-1) 0; }

  .markdown :global(blockquote) {
    margin: var(--space-3) 0;
    border-left: 2px solid var(--border-token);
    padding: var(--space-1) 0 var(--space-1) var(--space-3);
    color: var(--muted-foreground);
  }

  .markdown :global(pre) {
    overflow-x: auto;
    margin: var(--space-3) 0;
    border: 1px solid var(--border-token);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    background: var(--background);
  }

  .markdown :global(code) {
    border-radius: var(--radius-sm);
    padding: 0 var(--space-1);
    background: var(--muted-surface);
    color: var(--foreground);
    font-family: var(--terminal-font-family);
    font-size: var(--font-size-xs);
  }

  .markdown :global(pre code) { padding: 0; background: none; color: var(--foreground); }
  .markdown :global(a) { color: var(--ring); text-underline-offset: 3px; }
  .markdown :global(hr) { margin: var(--space-4) 0; border: 0; border-top: 1px solid var(--border-token); }
</style>

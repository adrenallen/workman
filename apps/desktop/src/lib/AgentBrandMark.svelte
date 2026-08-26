<script lang="ts">
  import type { AgentTool } from './agentTools';
  import { resolveAgentBrand } from './agentBrands';

  interface Props {
    tool?: AgentTool | null;
    fallbackName?: string;
    fallbackToolType?: string | null;
    size?: number;
    class?: string;
  }

  let {
    tool = null,
    fallbackName = 'Agent',
    fallbackToolType = null,
    size = 14,
    class: className = ''
  }: Props = $props();

  let brand = $derived(resolveAgentBrand(tool, fallbackName, fallbackToolType));
  let label = $derived(tool?.icon_data_url ? `${tool.name} custom mark` : brand.label);
</script>

<span
  class={`agent-brand-mark ${tool?.icon_data_url ? 'custom' : 'monogram'} ${className}`}
  style={`--agent-brand-size: ${size}px`}
  role="img"
  aria-label={label}
  title={label}
  data-agent-brand={brand.id}
>
  {#if tool?.icon_data_url}
    <img src={tool.icon_data_url} alt="" />
  {:else}
    <span aria-hidden="true">{brand.monogram}</span>
  {/if}
</span>

<style>
  .agent-brand-mark { display: inline-grid; width: var(--agent-brand-size); height: var(--agent-brand-size); flex: none; place-items: center; overflow: hidden; color: currentColor; opacity: 0.58; }
  .agent-brand-mark img { display: block; width: 100%; height: 100%; }
  .agent-brand-mark img { object-fit: contain; }
  .agent-brand-mark.monogram { border: 1px solid currentColor; border-radius: 3px; opacity: 0.48; }
  .agent-brand-mark.monogram > span { font: 700 max(6px, calc(var(--agent-brand-size) * 0.45))/1 'JetBrains Mono Variable', monospace; letter-spacing: -0.08em; transform: translateX(-0.25px); }
  .agent-brand-mark.custom { border-radius: 3px; opacity: 0.68; }
</style>

/** Keep desktop tool routing identical to workmand's normalize_tool_type. */
export function normalizeAgentToolType(toolType: string | null | undefined): string {
  return (toolType ?? '').trim().toLowerCase().replace(/[- ]/gu, '_');
}

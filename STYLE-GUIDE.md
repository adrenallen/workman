# Workman interface style guide

Workman is a dense, local-first developer instrument panel. It should feel closer to a precise desktop tool than a marketing site: quiet graphite surfaces, clear hierarchy, fast keyboard paths, and color reserved for state.

## Compose from the system

- Tailwind v4 and the shadcn-svelte components in `apps/desktop/src/lib/components/ui` are the component foundation. Do not add a second component library.
- `apps/desktop/src/styles.css` is the only source for color, type, radius, spacing, and focus tokens. Component CSS may own layout, but it must consume tokens instead of introducing hex colors.
- Use `cn()` from `$lib/utils` to merge classes. Use `tailwind-variants` for a component with multiple visual variants.
- Prefer an existing primitive before writing interaction code: `Button`, `Input`, `Textarea`, `Checkbox`, `Switch`, `Dialog`, `AlertDialog`, `Popover`, `Tabs`, `Tooltip`, `DropdownMenu`, `ContextMenu`, `ScrollArea`, `Command`, `Select`, and `Collapsible`.
- Shared Workman wrappers live in `components/ds`: `IconButton` for icon-only actions, `StatusIndicator` for semantic status, and `TooltipLabel` for compact metadata.

## Visual contract

- The base UI text is 14px. Navigation, tree rows, controls, and secondary copy must never be smaller than `--font-size-xs` (12px); primary row labels use at least `--font-size-sm` (13px).
- Use the 4px spacing grid (`--space-1` through `--space-4`). Keep rows compact, normally 28–36px high. Density comes from alignment and hierarchy, not tiny text.
- Use `--background`, `--card`, and `--popover` for the surface stack; `--border-token` and `--input` for edges; `--foreground` and `--muted-foreground` for copy.
- Blue `--ring` is focus only. Green `--success` means active work/healthy, amber `--warning-token` means attention, red `--destructive` means exited/crashed/destructive, and `--information` is reserved for a deliberate waiting/timer state. Do not use semantic colors as decoration.
- The standard radius is `--radius`. Avoid pills unless the value is genuinely a compact status or count.
- The signature treatment is a crisp blue focus frame on an otherwise quiet, low-contrast chrome. Do not add glow, gradients, glass effects, or ornamental motion.

## Icons, labels, and status

- Use Lucide Svelte icons from `@lucide/svelte/icons/*`, normally 14–16px with `strokeWidth={1.8}`. Do not use Unicode glyphs as interface icons.
- Icon-only controls must use `IconButton` and provide an action label plus any shortcut. Visible text wins when an action would be ambiguous.
- Every dot, badge, count, health light, or attention marker must have an exact tooltip and accessible label. Use `StatusIndicator`; never add a bare colored circle.
- Agent state always uses `AgentStatusIndicator`: gray static circle = idle, spinning green loader = working, amber alert = needs input, blue clock = waiting/timer, and red exit icon = exited. Exit tooltips distinguish clean exits from crashes.
- Status copy should answer what and why: “Daemon connected · port 62749”, “Agent needs input”, or “3 running of 5 terminals”. Avoid generic labels like “online” without a subject.

## Interaction contract

- Library primitives own focus trapping, Escape, outside-click dismissal, roving tab focus, and ARIA. Do not recreate those behaviors with document listeners.
- Preserve global keyboard paths: Cmd+K quick jump/create, Cmd+, Settings, Cmd+/ shortcuts, Cmd+B and Cmd+Shift+B panel collapse, Cmd+arrows navigation, Option+arrows reorder, and Cmd+U terminal unfocus.
- Terminal input is sovereign: application shortcuts must not intercept terminal keystrokes except the documented unfocus path.
- Keep project-rail and tree collapse/resize values persisted. Visual refactors must not change their storage keys.
- Empty states teach one next action using a primary `Button`; do not leave blank panels or decorative placeholder copy.

## Component example

```svelte
<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';
  import { Button } from '$lib/components/ui/button';
  import StatusIndicator from '$lib/components/ds/StatusIndicator.svelte';
</script>

<Button size="sm" onclick={createItem}><PlusIcon />New item</Button>
<StatusIndicator tone="success" label="Daemon connected · port 62749" />
```

## Review checklist

- `npm run check` and `npm run build` are clean.
- Keyboard-only navigation, focus return, Escape, and terminal passthrough still work.
- Text remains readable at 90%, 100%, 110%, and 120% app scale.
- Narrow, normal, collapsed, and resized panel states do not clip or overflow.
- Dark and light themes use the same semantic tokens.
- Every indicator and icon-only action exposes its meaning on hover and to assistive technology.
- Installed-app visual QA uses an isolated `WORKMAN_DATA_DIR`; no test daemon or app remains running.

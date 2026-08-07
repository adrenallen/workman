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
- Semantic state color comes only from the `--agent-state-*`, `--todo-state-*`, and `--notification-unread*` tokens in `apps/desktop/src/styles.css`. The map is gray = idle, green = working/healthy, violet = needs human input, orange = waiting/timer, amber = claimed ownership, blue = unread/notification only, and red = exited/crashed/destructive. Blue `--ring` remains a non-state keyboard-focus affordance; do not use blue for waiting, progress, or generic information.
- The standard radius is `--radius`. Avoid pills unless the value is genuinely a compact status or count.
- The signature treatment is a crisp blue focus frame on an otherwise quiet, low-contrast chrome. Do not add glow, gradients, glass effects, or ornamental motion.

## Icons, labels, and status

- Use Lucide Svelte icons from `@lucide/svelte/icons/*`, normally 14–16px with `strokeWidth={1.8}`. Do not use Unicode glyphs as interface icons.
- Icon-only controls must use `IconButton` and provide an action label plus any shortcut. Visible text wins when an action would be ambiguous.
- Every dot, badge, count, health light, or attention marker must have an exact tooltip and accessible label. Use `StatusIndicator`; never add a bare colored circle.
- Agent state always uses `AgentStatusIndicator`: gray static circle = idle, spinning green loader = working, violet alert = needs input, orange stroked clock on a transparent background = waiting/timer, and red exit icon = exited. Exit tooltips distinguish clean exits from crashes.
- Todo state always uses `TodoStatusIndicator` and the shared `--todo-state-*` tokens: a hollow neutral-gray circle means open/unclaimed, an amber circle-dot means claimed or in progress, a red alert means blocked, and a muted gray check means completed. Amber communicates active ownership without borrowing agent-working green or agent-waiting orange; its filled-square glyph separates claimed ownership from the adjacent stroked waiting clock, while violet keeps needs-input distinct from both.
- Status copy should answer what and why: “Daemon connected · port 62749”, “Agent needs input”, or “3 running of 5 terminals”. Avoid generic labels like “online” without a subject.
- Row status belongs in a persistent trailing zone. Reveal hover actions in a separate in-flow slot that yields label space; never place an opaque or absolutely positioned action layer over PR, agent, or health indicators.

## Interaction contract

- Library primitives own focus trapping, Escape, outside-click dismissal, roving tab focus, and ARIA. Do not recreate those behaviors with document listeners.
- Preserve global keyboard paths: Cmd+K quick jump/create, Cmd+, Settings, Cmd+/ shortcuts, Cmd+B and Cmd+Shift+B panel collapse, Cmd+arrows navigation, Option+arrows reorder, and Cmd+U terminal unfocus.
- Terminal input is sovereign: application shortcuts must not intercept terminal keystrokes except the documented unfocus path.
- Terminal canvas colors are a user-controlled subsystem, independent of the app's light/dark preference. Graphite (`#202326`) is the migration-free default: soft enough to avoid a stark black well, with warm off-white text and restrained ANSI hues. Palette hex values belong in the appearance model or imported profile data; terminal chrome outside the canvas still uses interface tokens.
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
- Installed-app visual QA launches through `scripts/native-visual-qa.sh` with a per-todo bundle ID. The staged bundle persists its fresh `/tmp` `WORKMAN_DATA_DIR`, `WORKMAN_CONFIG`, and explicit-daemon guard in `LSEnvironment`, so a Computer Use or LaunchServices reopen stays isolated; per-todo bundles fail closed if that contract is absent. No test daemon or app remains running.

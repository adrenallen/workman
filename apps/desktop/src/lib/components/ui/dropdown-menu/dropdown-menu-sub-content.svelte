<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		avoidCollisions = true,
		collisionPadding = 8,
		class: className,
		...restProps
	}: DropdownMenuPrimitive.SubContentProps = $props();
</script>

<DropdownMenuPrimitive.SubContent
	bind:ref
	data-slot="dropdown-menu-sub-content"
	{avoidCollisions}
	{collisionPadding}
	class={cn("min-w-[96px] !max-h-[calc(100vh-16px)] !max-w-[calc(100vw-16px)] scroll-py-1 rounded-lg bg-popover p-1 text-popover-foreground shadow-lg ring-1 ring-foreground/10 duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 z-50 w-auto overflow-x-hidden !overflow-y-auto overscroll-contain outline-none [scrollbar-color:var(--border-strong)_transparent] [scrollbar-width:thin]", className)}
	{...restProps}
/>

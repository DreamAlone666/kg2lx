<script lang="ts">
	import type { HTMLAttributes } from "svelte/elements";
	import { cn } from "$lib/utils.js";

	let { class: className, children, ...rest }: HTMLAttributes<HTMLDivElement> = $props();
</script>

<div
	class={cn("rounded-xl border bg-card text-card-foreground shadow", className)}
	{...rest}
>
	{@render children?.()}
</div>

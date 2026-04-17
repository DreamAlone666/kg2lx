<script lang="ts" module>
	import { type VariantProps, tv } from "tailwind-variants";

	export const buttonVariants = tv({
		base: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
		variants: {
			variant: {
				default:
					"bg-black text-white shadow hover:bg-black/90",
				destructive:
					"bg-red-600 text-white shadow-sm hover:bg-red-600/90",
				outline:
					"border border-input bg-background shadow-sm hover:bg-accent hover:text-accent-foreground",
				secondary:
					"bg-slate-100 text-slate-900 shadow-sm hover:bg-slate-100/80",
				ghost: "hover:bg-slate-100 hover:text-slate-900",
				link: "text-black underline-offset-4 hover:underline",
			},
			size: {
				default: "h-9 px-4 py-2",
				sm: "h-8 rounded-md px-3 text-xs",
				lg: "h-10 rounded-md px-8",
				icon: "h-9 w-9",
			},
		},
		defaultVariants: {
			variant: "default",
			size: "default",
		},
	});

	export type Variant = VariantProps<typeof buttonVariants>["variant"];
	export type Size = VariantProps<typeof buttonVariants>["size"];
</script>

<script lang="ts">
	import type { HTMLButtonAttributes, HTMLAnchorAttributes } from "svelte/elements";
	import { cn } from "$lib/utils.js";

	type Props = (HTMLButtonAttributes | HTMLAnchorAttributes) & {
		variant?: Variant;
		size?: Size;
		href?: string;
		ref?: any;
	};

	let {
		class: className,
		variant = "default",
		size = "default",
		href,
		ref = $bindable(null),
		children,
		...rest
	}: Props = $props();
</script>

{#if href}
	<a
		bind:this={ref}
		{href}
		class={cn(buttonVariants({ variant, size }), className)}
		{...(rest as HTMLAnchorAttributes)}
	>
		{@render children?.()}
	</a>
{:else}
	<button
		bind:this={ref}
		class={cn(buttonVariants({ variant, size }), className)}
		{...(rest as HTMLButtonAttributes)}
	>
		{@render children?.()}
	</button>
{/if}

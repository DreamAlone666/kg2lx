<script lang="ts" module>
	import { type VariantProps, tv } from 'tailwind-variants';

	/**
	 * 装饰性标签（如平台名、分类）。**不承载业务状态**。
	 *
	 * 状态必须使用 StatusBadge：那里有 tone 语义校验，能保证「同一状态永远是同一颜色」。
	 * 这里若用于表达状态，颜色就会退化成人各自理解的主观选择，失去信息价值。
	 */
	export const badgeVariants = tv({
		base: 'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors',
		variants: {
			variant: {
				default: 'border-transparent bg-primary text-primary-foreground',
				secondary: 'border-transparent bg-secondary text-secondary-foreground',
				destructive: 'border-transparent bg-destructive text-destructive-foreground',
				outline: 'border-border bg-background text-foreground',
			},
		},
		defaultVariants: {
			variant: 'secondary',
		},
	});

	export type Variant = VariantProps<typeof badgeVariants>['variant'];
</script>

<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils.js';

	let {
		class: className,
		variant = 'secondary',
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		variant?: Variant;
	} = $props();
</script>

<div class={cn(badgeVariants({ variant }), className)} {...rest}>
	{@render children?.()}
</div>

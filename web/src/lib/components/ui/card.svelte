<script lang="ts" module>
	import { tv } from 'tailwind-variants';

	/**
	 * 卡片阴影刻意保持极轻：层级主要由明度层次（页面 canvas #FAFAFA 与卡片
	 * bg-card #FFFFFF）承担，阴影只做「浮起」提示，避免后台式的厚重投影。
	 *
	 * interactive 用于整卡可点的场景（如音源卡片），提供 hover 抬升与键盘焦点环；
	 * 抬升使用 motion-safe，系统要求减弱动态时不播放位移。
	 */
	export const cardVariants = tv({
		base: 'rounded-xl border bg-card text-card-foreground shadow-soft',
		variants: {
			interactive: {
				true: 'group block transition-[box-shadow,border-color,transform] duration-200 ease-out-soft hover:border-primary/30 hover:shadow-lift motion-safe:hover:-translate-y-0.5',
				false: '',
			},
		},
		defaultVariants: { interactive: false },
	});
</script>

<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils.js';

	let {
		class: className,
		interactive = false,
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & { interactive?: boolean } = $props();
</script>

<div class={cn(cardVariants({ interactive }), className)} {...rest}>
	{@render children?.()}
</div>

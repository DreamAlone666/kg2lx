<script lang="ts">
	import { fly } from 'svelte/transition';
	import { toast } from '$lib/state/toast.svelte';
	import { useReducedMotion } from '$lib/hooks/use-reduced-motion.svelte';
	import { DURATION_BASE } from '$lib/utils/motion';
	import { cn } from '$lib/utils.js';
	import { toneSurfaceClass } from './tone-classes.js';

	/**
	 * 全局提示宿主。挂在根布局上，只渲染一次。
	 *
	 * 注意不能写成 `const { reduced } = useReducedMotion()`：解构会立刻求值一次
	 * getter，丢掉后续的响应式更新。必须保留对象再在响应式上下文里读取属性。
	 */
	const motion = useReducedMotion();

	// 减弱动态时不做位移也不淡入，仅瞬时切换
	const flyParams = $derived(
		motion.reduced
			? { y: 0, duration: 0, opacity: 1 }
			: { y: 12, duration: DURATION_BASE, opacity: 0 }
	);
</script>

<div
	class="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex flex-col items-center gap-2 p-4 sm:items-end sm:p-6"
	role="status"
	aria-live="polite"
>
	{#each toast.items as item (item.id)}
		<div
			transition:fly={flyParams}
			class={cn(
				'pointer-events-auto flex w-full max-w-sm items-center gap-2 rounded-lg border py-2 pr-2 pl-4 text-sm shadow-lift md:py-2.5 md:pr-3',
				toneSurfaceClass(item.tone)
			)}
		>
			<span class="flex-1 leading-snug">{item.message}</span>

			<!-- 触屏保证 44px 可点区域，桌面回落为紧凑尺寸 -->
			<button
				type="button"
				class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md text-base leading-none opacity-60 transition-opacity hover:opacity-100 md:size-7"
				aria-label="关闭提示"
				onclick={() => toast.dismiss(item.id)}
			>
				<span aria-hidden="true">&times;</span>
			</button>
		</div>
	{/each}
</div>

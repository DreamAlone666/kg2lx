<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils.js';

	/**
	 * 高度与圆角刻意与 Button 保持同一数阶，使表单内控件视觉基线对齐。
	 * 焦点环由 layout.css 的 :focus-visible 统一提供，此处不再叠加 ring，
	 * 避免出现「输入框一种焦点样式、按钮另一种」的不一致。
	 */
	let {
		class: className,
		value = $bindable(),
		ref = $bindable(null),
		...rest
	}: HTMLInputAttributes & {
		ref?: HTMLInputElement | null;
	} = $props();
</script>

<input
	bind:this={ref}
	bind:value
	class={cn(
		'flex h-11 w-full rounded-lg border border-input bg-background px-3 py-1 text-base transition-colors duration-150 ease-out-soft placeholder:text-muted-foreground hover:border-foreground/20 disabled:cursor-not-allowed disabled:opacity-50 md:h-10 md:text-body',
		className
	)}
	{...rest}
/>

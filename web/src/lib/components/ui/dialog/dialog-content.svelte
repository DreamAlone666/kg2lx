<script lang="ts">
	import { Dialog as DialogPrimitive } from 'bits-ui';
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils.js';

	/**
	 * 轻量浮层容器：bits-ui Dialog 的本仓包装（Portal + 滚动锁定 + 焦点陷阱 + Escape）。
	 *
	 * - center：移动端贴底圆角卡，sm+ 居中弹窗
	 * - drawer：移动端底部抽屉（max-h-[88dvh]），sm+ 右侧全高侧边抽屉（不与页面主滚动条抢滚轮、不挤压主表列宽）
	 */
	let {
		variant = 'center',
		class: className,
		children,
		...rest
	}: Omit<DialogPrimitive.ContentProps, 'class' | 'children'> & {
		variant?: 'center' | 'drawer';
		class?: string;
		children?: Snippet;
	} = $props();

	const layoutClass = $derived(
		variant === 'drawer'
			? 'fixed inset-x-0 bottom-0 top-auto z-50 flex max-h-[88dvh] flex-col overflow-hidden rounded-t-2xl border-t border-border bg-card text-card-foreground shadow-lift sm:inset-y-0 sm:right-0 sm:left-auto sm:max-h-none sm:w-full sm:max-w-lg sm:rounded-none sm:border-t-0 sm:border-l'
			: 'fixed inset-x-3 bottom-3 top-auto z-50 flex max-h-[85vh] flex-col overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-lift sm:inset-x-auto sm:top-1/2 sm:bottom-auto sm:left-1/2 sm:w-full sm:max-w-lg sm:-translate-x-1/2 sm:-translate-y-1/2'
	);
</script>

<DialogPrimitive.Portal>
	<DialogPrimitive.Overlay
		class={cn(
			'fixed inset-0 z-50',
			variant === 'drawer' ? 'bg-foreground/15 backdrop-blur-[1px]' : 'bg-foreground/25 backdrop-blur-xs'
		)}
	/>
	<DialogPrimitive.Content
		class={cn(layoutClass, className)}
		{...rest}
	>
		{@render children?.()}
	</DialogPrimitive.Content>
</DialogPrimitive.Portal>

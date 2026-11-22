<script lang="ts">
	import { Dialog as DialogPrimitive } from 'bits-ui';
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils.js';

	/**
	 * 轻量浮层弹窗容器：bits-ui Dialog 的本仓包装（Portal + 焦点陷阱 + Escape）。
	 *
	 * 用于移动端（< lg）的事件检查器等需要遮罩聚焦的场景；桌面端优先使用非遮挡分栏。
	 */
	let {
		class: className,
		children,
		...rest
	}: Omit<DialogPrimitive.ContentProps, 'class' | 'children'> & {
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<DialogPrimitive.Portal>
	<DialogPrimitive.Overlay
		class="fixed inset-0 z-50 bg-foreground/25 backdrop-blur-xs"
	/>
	<DialogPrimitive.Content
		class={cn(
			'fixed inset-x-3 bottom-3 top-auto z-50 flex max-h-[85vh] flex-col overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-lift sm:inset-x-auto sm:top-1/2 sm:bottom-auto sm:left-1/2 sm:w-full sm:max-w-lg sm:-translate-x-1/2 sm:-translate-y-1/2',
			className
		)}
		{...rest}
	>
		{@render children?.()}
	</DialogPrimitive.Content>
</DialogPrimitive.Portal>

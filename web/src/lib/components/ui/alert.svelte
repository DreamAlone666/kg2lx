<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Tone } from '$lib/domain/tone';
	import { cn } from '$lib/utils.js';
	import { toneSurfaceClass } from './tone-classes.js';

	/**
	 * 内联提示条 —— 错误、警告、成功等页面内反馈的统一形态。
	 *
	 * 提炼原因：同一段 `rounded-xl border p-4 + toneSurfaceClass(...)` 的拼装
	 * 曾散落在 6 处页面与组件里，各处 padding、圆角、文案层级都在缓慢漂移。
	 *
	 * 与 Toaster 的分工：本组件用于**与当前内容相关、需要留在页面上**的反馈
	 * （加载失败、绑定失败）；转瞬即逝的操作结果（复制成功）走全局提示。
	 */
	let {
		tone = 'danger',
		title,
		class: className,
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		tone?: Tone;
		/** 可选标题；省略时 children 直接作为正文 */
		title?: string;
	} = $props();
</script>

<div
	class={cn('rounded-xl border p-4 text-sm leading-relaxed', toneSurfaceClass(tone), className)}
	role="alert"
	{...rest}
>
	{#if title}
		<p class="font-medium">{title}</p>
		<div class="mt-1">{@render children?.()}</div>
	{:else}
		{@render children?.()}
	{/if}
</div>

<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Tone } from '$lib/domain/tone';
	import { cn } from '$lib/utils.js';
	import { toneChipClass, toneDotClass } from './tone-classes.js';

	/**
	 * 语义状态胶囊 —— 全站状态展示的唯一入口。
	 *
	 * 调用方只传 tone（由 $lib/domain/tone 从业务状态推导）与文本，
	 * 不自行拼颜色；这样「同一状态在任何页面必须是同一个颜色」才能被强制保证。
	 *
	 * 无障碍：状态不只用颜色区分，必须同时带文字（children 不可省略）。
	 */
	let {
		tone = 'neutral',
		dot = false,
		pulse = false,
		class: className,
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		tone?: Tone;
		/** 在文字前显示实心圆点，用于在密集表格里强化存在感 */
		dot?: boolean;
		/** 圆点呼吸，表示「进行中」这类持续状态 */
		pulse?: boolean;
	} = $props();
</script>

<div
	class={cn(
		'inline-flex items-center gap-1.5 whitespace-nowrap rounded-full px-2.5 py-0.5 text-xs font-medium',
		toneChipClass(tone),
		className
	)}
	{...rest}
>
	{#if dot}
		<span
			class={cn('size-1.5 shrink-0 rounded-full', toneDotClass(tone), pulse && 'animate-breathe')}
			aria-hidden="true"
		></span>
	{/if}
	{@render children?.()}
</div>

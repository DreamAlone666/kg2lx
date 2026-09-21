<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Tone } from '$lib/domain/tone';
	import { cn } from '$lib/utils.js';
	import { toneChipClass, toneChipSolidClass, toneDotClass } from './tone-classes.js';

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
		emphasis = 'soft',
		dot = false,
		pulse = false,
		class: className,
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		tone?: Tone;
		/**
		 * 胶囊强度。'soft' 是柔和底（默认，也是本组件在此参数出现前的唯一形态），
		 * 'solid' 是实心底，用于在同一色相里标出「更重要的那一条」——目前唯一的
		 * 调用方是最高一档会员权益（见 domain/tone.ts 的 vipEntitlementEmphasis）。
		 *
		 * 它只是强度开关，不是颜色开关：色相仍只由 tone 决定，'solid' 取的是同一个
		 * tone 的实心令牌（见 tone-classes.ts 的 toneChipSolid）。默认值保证既有
		 * 调用方一行不动、行为不变。
		 */
		emphasis?: 'soft' | 'solid';
		/** 在文字前显示实心圆点，用于在密集表格里强化存在感 */
		dot?: boolean;
		/**
		 * 圆点呼吸。**只用于「用户正在等待其结束」的状态**——例如扫码页的
		 * 「等待扫码」、日志的「刷新中」。
		 *
		 * 长期驻留的被动状态（如日志轮询开着时的「实时」）不加脉冲：用户无法据此
		 * 做任何动作，无上限的循环动画只是持续的分心源，动画成本大于信息价值。
		 */
		pulse?: boolean;
	} = $props();
</script>

<div
	class={cn(
		'inline-flex items-center gap-1.5 whitespace-nowrap rounded-full px-2.5 py-0.5 text-xs font-medium',
		emphasis === 'solid' ? toneChipSolidClass(tone) : toneChipClass(tone),
		className
	)}
	{...rest}
>
	{#if dot}
		<!--
			实心底上，圆点必须换成与底色相对的前景色才看得见：toneDot 给的是**同色**实心点，
			落在同色的实心胶囊上就成了底色的一部分，肉眼等同于圆点消失。这里不动 toneDot
			的取值——它在柔和底胶囊与时间轴节点上都是对的，只是不适用于实心底。
		-->
		<span
			class={cn(
				'size-1.5 shrink-0 rounded-full',
				emphasis === 'solid' ? 'bg-background' : toneDotClass(tone),
				pulse && 'animate-breathe'
			)}
			aria-hidden="true"
		></span>
	{/if}
	{@render children?.()}
</div>

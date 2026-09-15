<script lang="ts" module>
	import { type VariantProps, tv } from 'tailwind-variants';

	/**
	 * 尺寸采用 4px 数阶，并按断点区分「触屏」与「指针」两种输入方式：
	 *   - 移动端（默认）保持 >= 44px 的可点区域，满足触控工效下限
	 *   - md 及以上回落为紧凑高度，避免桌面端按钮显得笨重
	 *
	 * 焦点样式不在此处定义：layout.css 的 :focus-visible 统一兜底，
	 * 保证所有可交互元素（含卡片、链接）的焦点环完全一致。
	 *
	 * [TODO] 「触屏 >= 44px、md 以上回落紧凑」这条规则目前是手写复制的：除本文件四个
	 * 尺寸档外，(app)/+layout.svelte 的导航链接与品牌、LogsPanel 的日志视图切换、
	 * Toaster 的关闭按钮都各自手写了一遍 md:min-h-* / md:size-*，合计 8 处。
	 * 规则一旦调整需要逐处查找，漏改不会有任何提示——这与当初 5 处重复的中文映射
	 * 是同一类问题，只是对象从文案换成了尺寸。
	 * 方向：下沉为令牌层的 @utility（如 touch-target）或统一高度档位，只留一处定义。
	 */
	export const buttonVariants = tv({
		base: 'inline-flex cursor-pointer select-none items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium transition-[color,background-color,border-color,box-shadow,transform] duration-150 ease-out-soft disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0',
		variants: {
			variant: {
				default:
					'bg-primary text-primary-foreground shadow-soft hover:bg-primary-hover motion-safe:active:scale-[0.97]',
				destructive:
					'bg-destructive text-destructive-foreground shadow-soft hover:bg-destructive/90 motion-safe:active:scale-[0.97]',
				outline:
					'border border-input bg-background text-foreground shadow-soft hover:border-primary/30 hover:bg-primary-soft hover:text-primary-soft-foreground motion-safe:active:scale-[0.97]',
				secondary:
					'bg-secondary text-secondary-foreground hover:bg-secondary/70 motion-safe:active:scale-[0.97]',
				ghost: 'text-foreground hover:bg-accent hover:text-accent-foreground',
				link: 'text-primary underline-offset-4 hover:underline',
			},
			size: {
				default: 'h-11 px-4 md:h-10',
				sm: 'h-11 px-3.5 text-xs md:h-9',
				lg: 'h-12 px-6 md:h-11',
				icon: 'size-11 md:size-10',
			},
		},
		defaultVariants: {
			variant: 'default',
			size: 'default',
		},
	});

	export type Variant = VariantProps<typeof buttonVariants>['variant'];
	export type Size = VariantProps<typeof buttonVariants>['size'];
</script>

<script lang="ts">
	import type { HTMLButtonAttributes, HTMLAnchorAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils.js';

	type Props = (HTMLButtonAttributes | HTMLAnchorAttributes) & {
		variant?: Variant;
		size?: Size;
		href?: string;
		/** 异步动作进行中：切换文案、置 disabled、标记 aria-busy。 */
		pending?: boolean;
		/** 进行中时替换 children 的文案；省略则只置 disabled。 */
		pendingLabel?: string;
		ref?: HTMLButtonElement | HTMLAnchorElement | null;
	};

	/**
	 * pending 是项目里已被手写三遍的模式——「刷新中…」「连接中…」「获取中…」
	 * 各自维护一份「切文案 + 置 disabled」，且总有一处漏掉。在这里声明一次。
	 *
	 * 刻意不配旋转图标：layout.css 的减弱动态规则会把 animate-spin 一并压平，
	 * 只靠动画表达「进行中」会让这类用户看不到任何变化。文案切换与 disabled
	 * 是静态可辨识的，符合「加载指示不得只依赖动画」这条既有约束。
	 *
	 * 仅对 <button> 生效：<a> 没有 disabled 语义，传 href 时忽略本属性。
	 */
	let {
		class: className,
		variant = 'default',
		size = 'default',
		href,
		pending = false,
		pendingLabel,
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
		disabled={pending || (rest as HTMLButtonAttributes).disabled}
		{...(pending ? { 'aria-busy': 'true' } : {})}
	>
		{#if pending && pendingLabel}
			{pendingLabel}
		{:else}
			{@render children?.()}
		{/if}
	</button>
{/if}

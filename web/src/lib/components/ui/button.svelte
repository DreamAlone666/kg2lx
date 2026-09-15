<script lang="ts" module>
	import { type VariantProps, tv } from 'tailwind-variants';

	/**
	 * 尺寸采用 4px 数阶，并按断点区分「触屏」与「指针」两种输入方式：
	 *   - 移动端（默认）保持 >= 44px 的可点区域，满足触控工效下限
	 *   - md 及以上回落为紧凑高度，避免桌面端按钮显得笨重
	 *
	 * 焦点样式不在此处定义：layout.css 的 :focus-visible 统一兜底，
	 * 保证所有可交互元素（含卡片、链接）的焦点环完全一致。
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
		ref?: HTMLButtonElement | HTMLAnchorElement | null;
	};

	let {
		class: className,
		variant = 'default',
		size = 'default',
		href,
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
	>
		{@render children?.()}
	</button>
{/if}

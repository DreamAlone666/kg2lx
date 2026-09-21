<script lang="ts">
	import { Tooltip as TooltipPrimitive } from 'bits-ui';
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils.js';

	/**
	 * Tooltip 的浮层内容（`Tooltip.Content` 的本仓实现）。
	 *
	 * 为什么用原语而不是原生 `title`：
	 * - 原生 title 的显示延迟、字号、位置都由浏览器决定，无法与页面样式统一；它也
	 *   不跟随焦点事件之外的任何语义，读屏用户拿到的时机同样不可控；
	 * - 原生 title 只能放纯文本，且它**只**是补丁——本仓这三处的真实诉求是
	 *   「被 truncate 的值还能被看到」，需要的是一个能定位到触发元素旁的浮层。
	 *   本仓另有约 9 处 title 刻意保留（见 LogsPanel / SourceCard 的注释）：那里
	 *   加 trigger 会把日志表的 tab 停从 0 涨到几十个，键盘导航反而报废。
	 *
	 * 表面样式用**浅色浮起**（bg-card + border + shadow-lift），不照抄上游 2025-12 起
	 * 默认的 `bg-foreground text-background`：那是一个近黑面，会成为全站唯一的重面，
	 * 与本仓「层级靠明度层次表达、阴影只做极轻浮起」的定位（见 layout.css 令牌层）
	 * 相冲突。浮层只需要比底下的内容「亮一层」并被阴影托起来。
	 *
	 * `z-50` 与 Toaster 同值：浮层被 Portal 到 body，与顶栏（z-40）处在同一条
	 * 层叠上下文链上，必须高于它。注意这个类不是装饰——bits-ui 会读取内容节点的
	 * 计算 z-index 并同步到真正定位的 wrapper 上（wrapper 带 transform，本身就是
	 * 层叠上下文），所以 z-index 只能写在这里，写在调用方或 wrapper 上都无效。
	 */
	let {
		class: className,
		side = 'top',
		sideOffset = 6,
		children,
		...rest
	}: Omit<TooltipPrimitive.ContentProps, 'class' | 'children'> & {
		class?: string;
		/**
		 * 提示内容。本轮三处都是「被截断的那个值本身」，所以只收文本。
		 * 需要排版（多行键值）的场合应另做「日志行详情视图」，而不是把结构塞进提示里。
		 */
		children?: Snippet;
	} = $props();
</script>

<!--
	Portal 放在原语内部，调用方不写：三处触发点都在页面的滚动/裁剪容器里
	（详情页页头、脚本地址栏都在带 overflow 的布局中），不 Portal 会被祖先
	`overflow-y-auto` 直接裁掉；让调用方记得写正是本仓踩过的「约定靠记忆维持」的坑。
-->
<TooltipPrimitive.Portal>
	<!--
		sideOffset 6px：默认 0 会让浮层贴死触发元素边缘，像与文字连成一体。
		avoidCollisions 保持上游默认开启：页头那处在视口顶部，浮层会被翻到下方。
	-->
	<TooltipPrimitive.Content
		{side}
		{sideOffset}
		class={cn(
			'z-50 max-w-xs break-words rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs leading-snug text-foreground shadow-lift',
			className
		)}
		{...rest}
	>
		{@render children?.()}
	</TooltipPrimitive.Content>
</TooltipPrimitive.Portal>

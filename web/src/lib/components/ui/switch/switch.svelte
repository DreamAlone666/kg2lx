<script lang="ts">
	import { Switch as SwitchPrimitive } from 'bits-ui';
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils.js';

	/**
	 * 开关原语：bits-ui `Switch.Root` / `Switch.Thumb` 的本仓包装。
	 *
	 * 为什么换成原语，而不是继续手写 `<button role="switch" aria-checked>`：
	 * 手写版本只声明了角色与状态，行为得自己补齐——Space/Enter 的切换、按下期间
	 * 不重复触发、以及将来若需要提交值时的隐藏 input。这些都在 Root 里。
	 * 另一个收益是「外观与状态」在控件内部同源：Root 与 Thumb 各自都带
	 * `data-state="checked|unchecked"`，样式挂在属性上，轨道位置、轨道颜色与
	 * aria-checked 由同一个状态派生，不会出现「类名按一个值算、aria 按另一个值算」。
	 *
	 * 几何刻意保留本仓尺寸（轨道 36×20、滑块 14px，滑块与轨道边缘的内缩 3px），
	 * 不抄上游的 h-[1.15rem] / w-8 + 2px 内缩：这处观感是既定的，本轮引入原语
	 * 只应该修掉位移算错的问题，不应该顺带改设计。滑块位移的算式见下方注释。
	 *
	 * 表面色仍由本仓令牌决定：开启是 success（状态色系统，与 accountStatusTone 把
	 * active 判成 success 同一条映射），关闭是 neutral/40。强调色（暖橙）不参与——
	 * 它只用于主操作、当前导航项与焦点环。
	 *
	 * 焦点环由 layout.css 的全局 `:focus-visible` 提供，此处不叠加 ring，
	 * 与 Input 的做法一致（避免「按钮一种焦点样式、开关另一种」）。
	 *
	 * **受控契约**：`checked` 是双向的（`$bindable`）。用户拨动时 bits-ui 会先把新值
	 * 写回 `checked` 并调用 `onCheckedChange`，也就是说外观**先**变成新状态。若调用方
	 * 不接受这个新值（例如保存失败、服务端事实没变），必须自己把 `checked` 写回原值——
	 * 单向传一个没变的 `checked` 是没用的：值相同不会触发更新，控件会停在乐观位置上。
	 * 需要「保存收口后再对齐」的写法见 QualityFallbackSetting 的 checked 说明。
	 */
	let {
		class: className,
		checked = $bindable(false),
		children,
		...rest
	}: Omit<SwitchPrimitive.RootProps, 'class' | 'children'> & {
		/** 外壳类。轨道与滑块的几何是原语的一部分，不从调用方传。 */
		class?: string;
		/**
		 * 开关右侧的说明文字。刻意用 children 而不是 `label` 字符串：文案要能随
		 * 调用方状态变化（本仓是「已开启 / 已关闭 / 保存中…」），而且它必须留在
		 * Root 渲染的那颗 `<button>` 内部——移出去就不再是点击区域的一部分，
		 * 触控目标会缩水到轨道那 36×20。
		 */
		children?: Snippet;
	} = $props();
</script>

<!--
	Root 就是那颗可聚焦、可点击的 button；轨道是它内部一个 aria-hidden 的装饰性 span。
	`group` 供轨道读取 Root 上的 data-state（轨道不是 Thumb，拿不到自己的 state）。
	`type` 由 bits-ui 默认为 "button"，无需再写（开关不参与表单提交）。
-->
<SwitchPrimitive.Root
	bind:checked
	class={cn(
		'group inline-flex shrink-0 cursor-pointer items-center gap-2 rounded-lg px-1 transition-colors duration-150 ease-out-soft disabled:cursor-not-allowed disabled:opacity-50',
		className
	)}
	{...rest}
>
	<span
		class="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors duration-200 ease-out-soft group-data-[state=checked]:bg-success group-data-[state=unchecked]:bg-neutral/40"
		aria-hidden="true"
	>
		<!--
			滑块位移算式：轨道 36px − 滑块 14px − 尾侧内缩 3px = 19px = 1.1875rem。

			首侧的 3px 不由位移承担：滑块是绝对定位且没有 left/top，静态位置就落在轨道
			左缘（0），关闭态那 3px 由 translate-x-[0.1875rem] 给出。所以这里是减**一次**
			3px，不是减两次——写成 36 − 14 − 2×3 会得到 16px，滑块右缘仍会差 3px。

			此前开启态写的是 translate-x-[1.375rem]（22px = 36 − 14），把那 3px 内缩
			算成了 0，于是滑块右缘贴死轨道右缘（实测 左22/右0）；而关闭态是
			translate-x-[0.1875rem]（3px → 实测 左3/右19）。同一个错误在两端表现不同，
			只看关闭态就会以为几何是对的。
		-->
		<SwitchPrimitive.Thumb
			class="absolute size-3.5 rounded-full bg-background shadow-soft transition-transform duration-200 ease-out-soft data-[state=checked]:translate-x-[1.1875rem] data-[state=unchecked]:translate-x-[0.1875rem]"
		/>
	</span>
	{@render children?.()}
</SwitchPrimitive.Root>

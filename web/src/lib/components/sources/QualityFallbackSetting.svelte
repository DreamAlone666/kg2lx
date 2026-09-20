<script lang="ts">
	import { cn } from '$lib/utils.js';

	/**
	 * 音质自动降级开关（音源级配置）。
	 *
	 * 这个组件只负责**呈现与提交**，策略本身完全在服务端：渲染在用户机器上的 LX 脚本
	 * 是一个冻结产物，里面没有任何降级逻辑，脚本也无权改变宿主记录的档位。因此：
	 *
	 * - 拨动开关**即时生效**，不需要重新导入脚本（这一点必须写出来，否则用户会默认
	 *   认为「改了配置就要重新导入」——重导入是这一页上另一件事的代价，见「脚本地址」块）；
	 * - 代价必须明写在页面上，不能藏进 title：LX 无法告诉脚本本次是**播放**还是**下载**
	 *   （脚本拿到的只有 type 与 musicInfo），所以降级无法只作用于播放。把它塞进悬停
	 *   提示等于让用户在不知情的情况下接受一个会落到下载文件上的行为改变。
	 *
	 * 控件形态：本仓库没有 switch 原语，也没有 tooltip 原语。按既有惯例用原生
	 * `<button role="switch" aria-checked>`（角色与状态都由浏览器暴露给辅助技术），
	 * 说明文字用行内文本而不是 tooltip。
	 *
	 * 状态不只用颜色表达：轨道位置、颜色与「已开启 / 已关闭」文字三者同时变化。
	 *
	 * 无障碍命名走 `aria-labelledby` 指向可见标题，而不是再写一份 `aria-label`：
	 * 后者会盖掉按钮内的可见文字，屏幕阅读器听到的名字将与页面上看到的标题分叉。
	 */
	let {
		value,
		pending = false,
		disabled = false,
		onChange
	}: {
		value: boolean;
		pending?: boolean;
		/**
		 * 另有动作正在进行（详情页的「刷新 Token」），此刻的拨动会被那次动作回填的旧值冲掉。
		 *
		 * 与 `pending` 分开而不是并成一个：`pending` 会改文案（「保存中…」），而这里
		 * **没有在保存**——把它算进 `pending` 等于让开关报告一件没发生的事。禁用只负责挡住
		 * 这次拨动，文案照旧。
		 */
		disabled?: boolean;
		onChange: (next: boolean) => void;
	} = $props();

	/** 每次实例化唯一，避免同页多次挂载时 id 撞车。 */
	const uid = $props.id();
	const titleId = `quality-fallback-title-${uid}`;
	const descId = `quality-fallback-desc-${uid}`;
	const costId = `quality-fallback-cost-${uid}`;
</script>

<div class="space-y-3">
	<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
		<div class="min-w-0 max-w-prose space-y-1">
			<h3 id={titleId} class="text-sm font-medium">音质自动降级</h3>
			<p id={descId} class="text-xs leading-relaxed text-muted-foreground">
				开启后，当酷狗明确拒绝当前音质时会自动改请求更低档位，尽量让你能听到。
			</p>
			<!--
				代价单独成行、用 warning 色：它是一条用户必须知情的行为改变，不是补充说明。
				这里刻意给出**原因**（拿不到播放/下载的区分），否则「为什么下载也受影响」
				在界面上无从追问——一句没有理由的警告只会被当成免责声明划过去。

				它同时进 aria-describedby：用读屏操作的用户也该在拨动之前听到这条代价，
				而不是只有视觉用户从颜色里注意到它。
			-->
			<p id={costId} class="text-xs leading-relaxed text-warning">
				<span class="font-medium">下载同样生效，且客户端仍按所选音质显示</span>——你会拿到一个比所选档位更低的文件。洛雪无法告诉音源脚本本次是播放还是下载，所以降级无法只作用于播放。
			</p>
			<p class="text-xs leading-relaxed text-muted-foreground">
				默认关闭；改动即时生效，不需要重新导入脚本。
			</p>
		</div>

		<!--
			min-h-11 / md:min-h-0：与同页其它按钮的触控区下限对齐（移动端 44px、md 以上按内容）。

			pending 时**禁用**而不是只降透明度：不禁用的话连点会发出两次互相覆盖的 PATCH。
			disabled（另一个动作在进行）同样禁用，但**不改文案**：那一刻没有在保存。

			代价是这一档会随请求一起卡住（HttpClient 没有超时，请求不落地就一直是 pending）。
			仍然选它，是因为这里的卡死有出路：开关显示「保存中…」这个静态可辨识形态，
			而页面其余部分（导航、日志面板）都不受它影响——与本仓两次栽过的「表单把
			Escape 与取消一起禁掉、用户被困在里面」不是同一类情形。
		-->
		<button
			type="button"
			role="switch"
			aria-checked={value}
			aria-labelledby={titleId}
			aria-describedby="{descId} {costId}"
			disabled={pending || disabled}
			onclick={() => onChange(!value)}
			class="inline-flex min-h-11 shrink-0 cursor-pointer items-center gap-2 rounded-lg px-1 text-xs font-medium transition-colors duration-150 ease-out-soft disabled:cursor-not-allowed disabled:opacity-50 md:min-h-0"
		>
			<!--
				开启态用 success 而不是强调色：暖橙在本仓只用于主操作、当前导航项与焦点环
				（见 layout.css 的令牌层注释），而「已开启」是一个**状态**，归状态色系统管，
				accountStatusTone 把 active 判成 success 用的就是同一条映射。
			-->
			<span
				class={cn(
					'relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors duration-200 ease-out-soft',
					value ? 'bg-success' : 'bg-neutral/40'
				)}
				aria-hidden="true"
			>
				<span
					class={cn(
						'absolute size-3.5 rounded-full bg-background shadow-soft transition-transform duration-200 ease-out-soft',
						value ? 'translate-x-[1.375rem]' : 'translate-x-[0.1875rem]'
					)}
				></span>
			</span>
			<!--
				pending 文案是静态可辨识形态，不依赖动画：prefers-reduced-motion 会把
				过渡压平，届时若反馈只剩轨道位移，用户会以为点击没被接住。
			-->
			<span class="text-foreground">{pending ? '保存中…' : value ? '已开启' : '已关闭'}</span>
		</button>
	</div>
</div>

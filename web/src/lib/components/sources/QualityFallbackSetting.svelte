<script lang="ts">
	import { untrack } from 'svelte';
	import { Switch } from '$lib/components/ui/switch';

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
	 * 控件形态：用 `$lib/components/ui/switch`——引入 bits-ui 之后本仓有了开关原语
	 * （此前是手写的 `<button role="switch" aria-checked>`，它只声明了角色与状态，
	 * 键盘与指针行为都得自己补齐）。两处**刻意不变**：
	 *
	 * - 说明文字仍是行内文本，不是 tooltip：代价必须常驻可见（见上）。
	 * - 轨道与滑块的几何仍是本仓尺寸（36×20 / 滑块 14px / 内缩 3px），不随原语换成
	 *   上游的 16px 轨道——观感已定，本轮只修位移算错，不改设计。
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

	/**
	 * 开关的**显示值**，与原语双向绑定（`bind:checked`）。
	 *
	 * 为什么不把 `value` 直接交给原语：bits-ui 的 Switch 会把用户这一次拨动先落在
	 * 自己那一侧的状态里（Radix 系控件的通例），外观随后跟着它走。若只是单向传入
	 * `value`，保存失败时两者会分叉——实测过：滑块停在右侧、轨道变绿、aria-checked=true，
	 * 而旁边文字仍写着「已关闭」（控制器不做乐观更新，value 没变，原语也就没收到任何回拉）。
	 * 那正是本组件注释里说要避免的「留下一个没生效的已开启」。
	 *
	 * 收口规则：`value`（服务端事实）是唯一来源，**一次保存结束就以它为准**。
	 * pending 由真转假即这一刻——成功时 value 已是新值（写入幂等），失败时把外观拉回原值。
	 * 保存进行中刻意不干预：让乐观位置先顶着，否则请求期间开关纹丝不动，用户会以为没点中
	 * （与下面「保存中…」那条静态反馈配套）。
	 *
	 * `untrack` 只用在初值上：首帧必须直接是服务端事实（挂载后再靠 effect 补会有一次
	 * 反向闪动），后续同步交给下面那个 effect——它才是持有依赖的一方。
	 */
	let checked = $state(untrack(() => value));

	$effect(() => {
		if (pending) return;
		checked = value;
	});

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
		<Switch
			bind:checked
			onCheckedChange={onChange}
			disabled={pending || disabled}
			aria-labelledby={titleId}
			aria-describedby="{descId} {costId}"
			class="min-h-11 text-xs font-medium md:min-h-0"
		>
			<!--
				pending 文案是静态可辨识形态，不依赖动画：prefers-reduced-motion 会把
				过渡压平，届时若反馈只剩轨道位移，用户会以为点击没被接住。

				轨道与滑块的配色、以及「开启用 success 而不是强调色」的理由，
				都随几何一起收在原语里（见 ui/switch/switch.svelte）。
			-->
			<span class="text-foreground">{pending ? '保存中…' : value ? '已开启' : '已关闭'}</span>
		</Switch>
	</div>
</div>

<script lang="ts">
	import { page } from '$app/state';
	import {
		Alert,
		Button,
		Card,
		CardContent,
		CardHeader,
		Skeleton,
		StatusBadge
	} from '$lib/components/ui';
	import CopyField from '$lib/components/sources/CopyField.svelte';
	import SourceTitle from '$lib/components/sources/SourceTitle.svelte';
	import QualityFallbackSetting from '$lib/components/sources/QualityFallbackSetting.svelte';
	import LogsPanel from '$lib/components/source-logs/LogsPanel.svelte';
	import { sourcesController } from '$lib/features/sources/controller.svelte';
	import { toast } from '$lib/state/toast.svelte';
	import {
		accountStatusLabel,
		accountStatusTone,
		describeAccountError,
		entitlementExpiringTone,
		formatDateTime,
		isNamedVipProduct,
		labelOf,
		vipEntitlementEmphasis,
		VIP_ENTITLEMENT_TONE,
		vipProductLabel
	} from '$lib/domain';

	let sourceId = $derived(page.params.sourceId ?? '');
	/**
	 * 只渲染与 URL 对齐的那一个音源。
	 *
	 * 控制器是模块级单例，`currentSource` 在切换音源时不会立刻清空：从列表点进另一个
	 * 音源，在新数据回来之前它仍是上一个。直接渲染的话，用户会在 B 的地址上看到 A 的
	 * 标题与 A 的脚本地址；而改名功能上线后更糟——`{#key sourceId}` 会把 A 的 source
	 * 交给 B 的组件实例，预填出 A 的标题并把它写进 B。慢后端下这个窗口有数秒宽。
	 * 对齐失败一律当作「还没加载」，交给下面的骨架屏分支。
	 */
	let source = $derived(
		sourcesController.currentSource?.source_id === sourceId
			? sourcesController.currentSource
			: null
	);
	let refreshing = $derived(Boolean(sourcesController.refreshing[sourceId]));
	let pageTitle = $derived(source ? `${source.name} | kg2lx` : '音源详情 | kg2lx');

	/**
	 * 会员权益明细，`undefined` 表示**接口里没有这个键**。
	 *
	 * 不要把 undefined 折叠成空数组（`?? []`）：键不存在时服务端什么都没说（二进制与
	 * web/build 是两个可分别替换的东西），此时整段一个字都不该说；只有键存在而筛完没有
	 * 在期条目，才是「无在期记录」这个可以如实陈述的事实。详见 types.ts 里该字段的说明。
	 */
	let entitlements = $derived(source?.account.entitlements);

	/**
	 * 本次访问内是否改过标题。
	 *
	 * 洛雪音乐在导入时就固定了脚本头部，改标题不会回传给它，所以改完要提示用户重新导入。
	 * 但这条提示只在「刚刚改过」时才有意义——若换成任何「当前是否处于自定义态」的判断，
	 * 几个月前改过名的用户每次进来都会看到它，提示就退化成噪音。
	 */
	let renamedThisVisit = $state(false);

	// 认证守卫已上提到 (app)/+layout.ts，页面只负责取数
	$effect(() => {
		const id = page.params.sourceId;
		if (!id) return;
		renamedThisVisit = false;
		sourcesController.loadSource(id);
	});

	async function handleRefresh() {
		const result = await sourcesController.refreshSource(sourceId);
		if (result === 'ok') {
			toast.success('账号状态已刷新。');
		} else if (result === 'failed') {
			toast.error(sourcesController.error ?? '刷新失败，请稍后重试。');
		}
		// 'busy' 刻意不出提示：用户并没有遭遇失败，只是上一次刷新还在进行中。
		// 此前这里把 busy 与 failed 一起提示成「刷新失败，请稍后重试」，是在报告一件没发生的事。
	}

	/**
	 * 拨动音质降级开关。
	 *
	 * 与改名同一套反馈策略：失败刻意不弹提示——sourcesController.error 已被渲染成
	 * 页面横幅，再弹一条等于把同一句话讲两遍。开关自身在失败时保持原值（控制器不做
	 * 乐观更新），所以横幅消失后页面上不会留下一个没生效的「已开启」。
	 */
	function handleQualityFallbackChange(next: boolean) {
		void sourcesController.setQualityFallback(sourceId, next);
	}
</script>

<svelte:head>
	<title>{pageTitle}</title>
</svelte:head>

<div class="space-y-0">
	{#if !source && sourcesController.loading}
		<Card class="p-6">
			<Skeleton class="h-7 w-40" />
			<Skeleton class="mt-3 h-4 w-24" />
			<Skeleton class="mt-6 h-14 w-full" />
			<Skeleton class="mt-4 h-10 w-full" />
		</Card>
	{:else if !source}
		<Alert title="加载音源失败">
			<p class="leading-relaxed">
				{sourcesController.error ?? '未找到该音源，可能已被删除。'}
			</p>
			<Button variant="outline" size="sm" class="mt-4" href="/sources">返回音源列表</Button>
		</Alert>
	{:else}
		{#if sourcesController.error}
			<Alert class="mb-4">
				<div class="flex items-start justify-between gap-4">
					<p class="leading-relaxed">{sourcesController.error}</p>
					<Button variant="ghost" size="sm" onclick={() => sourcesController.clearError()}>
						关闭
					</Button>
				</div>
			</Alert>
		{/if}

		<Card style="view-transition-name: source-{sourceId}">
			<CardHeader class="border-b border-border/70">
				<div class="flex flex-wrap items-center justify-between gap-4">
					<!--
						{#key} 不可省：同一路由只换 sourceId 时 SvelteKit 会复用组件实例，
						半途的草稿与编辑态会原样留下，表现为「B 的页头里显示着 A 的草稿」。
					-->
					{#key sourceId}
						<SourceTitle {source} onRenamed={() => (renamedThisVisit = true)} />
					{/key}
					<!--
						保存标题或保存降级开关期间都禁用刷新：刷新会 loadSource + loadSources
						回填整页，把刚写入的值静默盖回旧值——用户会看到标题/开关「自己变回去了」。
						反方向的守卫：改名在 SourceTitle 内（刷新中禁用保存），开关则是下面把
						`refreshing` 传进它的 `disabled`——同一条理由，两个方向都要有，只堵一边
						就只能防住一半。
					-->
					<Button
						variant="outline"
						pending={refreshing}
						pendingLabel="刷新中…"
						disabled={sourcesController.savingName || sourcesController.savingQualityFallback}
						onclick={handleRefresh}
					>
						刷新 Token
					</Button>
				</div>
			</CardHeader>

			<CardContent class="space-y-8 p-6">
				<!--
					三段信息栅格：轨道数必须与内容数一致。
					此前是 4 列，第 4 格「音源 ID」是为了填满 5xl 宽度而加的内部主键——
					用无效内容凑版式，且被 truncate 后连完整读取都做不到。移除后把轨道数
					一并降到 3，才不会留下死轨道；地址栏里本来就是同一个 ID。

					后来又按同一条判据移掉了「VIP 状态」那一格：它是一个账号级的聚合布尔，而账号
					权益本身是并列的增量信息——聚合值既回答不了「有哪几项」也回答不了「哪一项快
					到期」，留着就是同一件事的第二套说法。权限改由下面独立成段的「会员权益」
					承担，轨道数随之降到 2。

					这条不变量对后续改动同样成立：新内容不够凑满一行时，**不要**往这里塞一格。
					音质降级开关就是按这条办的——它是可配置项而不是只读事实，下一块独立成段。
				-->
				<div class="grid grid-cols-2 gap-6">
					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">状态</p>
						<StatusBadge tone={accountStatusTone(source.account.status)} dot>
							{labelOf(accountStatusLabel, source.account.status)}
						</StatusBadge>
					</div>

					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							最后刷新时间
						</p>
						<!--
							窄屏用 text-xs 而不是 text-sm：19 字符的定长时间戳在 390px 的两列栅格里
							只有约 143px 可用，14px 下必然折行；12px 恰好单行放下，也让本格与同排的
							两个 StatusBadge（固定 text-xs）在字号上一致。
						-->
						<p class="tabular text-xs whitespace-nowrap sm:text-sm">
							{formatDateTime(source.account.last_refresh_at)}
						</p>
					</div>
				</div>

				<!--
					会员权益明细。**刻意独立成段**，不挤进上面那个三段栅格：那里把「轨道数 ==
					内容数」写成了不变量，而这里的条目数随账号变化（0..N），塞进去必然留下
					装不满的轨道。

					这一段要解决的是「权益会静默过期」：到期后账号一切正常、只是部分歌取不到
					高音质，而账号状态与登录态在此期间毫无变化，用户无从解释
					「为什么某些歌突然不能听了」。所以这里给的是逐条的权益名称与到期时刻。只列**有名字的**权益（见 isNamedVipProduct 的白名单说明）：没有名字的权益位不上榜，否则「即将到期」会被 1 天的日结条目常亮撑着，把真正的预警淹掉。

					整段以 entitlements 这个键**存在**为前提：键不存在时服务端什么都没说
					（二进制与 web/build 可分别替换），此时一个字都不说，而不是渲染成空态——
					空态是一句「没有在期权益」的断言，缺字段的场合我们无权做这个断言。
				-->
				{#if entitlements !== undefined}
						{@const listedEntitlements = entitlements.filter(
							(entitlement) =>
								entitlement.active && isNamedVipProduct(entitlement.product_type)
						)}
					<div class="space-y-3">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							会员权益
						</p>

						{#if listedEntitlements.length === 0}
							<!--
								这里能说「没查到」是因为键存在（服务端确实给了一张明细表），而且声明的
								范围只有白名单里那两条——所以空态说的是「没有它们」，不是「没有任何权益」。

								也**不说「未激活」**：账号级是否会员这类聚合结论已经删掉了（它既冗余、
								又回答不了「有哪几项」），本段没有任何可对照的东西，也就无从替它断言账号
								状态；登录态是否正常记在上面那一格，与本段无关。

								必须给出下一步动作：空态有两个成因（快照太旧 / 确实没有），两者该做的事
								不同；只说事实的话，用户看完仍然不知道该做什么——这是端到端验收时实际
								撞到的情形。
							-->
							<p class="text-xs leading-relaxed text-muted-foreground">
								未检测到概念版VIP / 畅听VIP 的在期记录。这份明细只在重新读取上游时更新，点上方「刷新 Token」可重读。
							</p>
						{:else}
							<!--
								不显示「剩余 N 天」这类推算值：到期时刻是事实，剩余天数会随时间变旧，
								而本页没有任何计时器会去刷新它（二维码那种短时交互才配一个）。

								布局用 flex-wrap 而不是 `grid grid-cols-2`：条目数随账号变化（0..N，
								见上文），固定两列的栅格在只有一条权益时就会留下一条装不满的死轨道——
								这正是上面那个三段栅格把「轨道数 == 内容数」立成不变量的理由，同一个
								理由在这里同样成立。flex 没有轨道、只有内容，才是这一段的正确形状。

								两条权益因此在宽屏落到同一行（块高 2 行 → 1 行），但**右侧空白不会消失**：
								这一行 1182px 里只有约 460px 是内容（权益名 + 到期时刻）。这条改动解决的是
								「此前是裸文本、没有任何视觉锚点，两条权益也看不出高低」，**不是**「把宽度
								填满」——把「右边还有空白」当成没做完，是误读了它的目的。

								权益名用 StatusBadge（与列表卡片同一个组件、同一个色调、同一个强度判据）：
								此前这里是裸 <span class="text-sm font-medium">，于是同一份数据在列表上是
								琥珀胶囊、在详情页是一行纯文本——同一条权益在两页有两种形态，正是本仓禁止
								的口径分叉。强调强度：最高一档实心、其余柔和底（VIP_ENTITLEMENT_TONE 只
								决定色相，强度由 vipEntitlementEmphasis 决定，见 domain/tone.ts）。
							-->
							<ul class="flex flex-wrap items-center gap-x-6 gap-y-2">
								{#each listedEntitlements as entitlement}
									{@const expiringTone = entitlementExpiringTone(entitlement.end_at)}
									<li class="flex flex-wrap items-center gap-x-3 gap-y-1">
										<StatusBadge
											tone={VIP_ENTITLEMENT_TONE}
											emphasis={vipEntitlementEmphasis(entitlement.product_type)}
										>
											{labelOf(vipProductLabel, entitlement.product_type)}
										</StatusBadge>
										<!--
											到期时刻复用同页「最后刷新时间」的 formatDateTime（不另造格式）；
											定长数字同样要 tabular + nowrap，否则多行之间无法纵向对齐、窄屏还会折行。
										-->
										<span class="tabular text-xs whitespace-nowrap text-muted-foreground">
											到期 {formatDateTime(entitlement.end_at, '未知')}
										</span>
										{#if expiringTone}
											<StatusBadge tone={expiringTone}>即将到期</StatusBadge>
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</div>
				{/if}

				<!--
					音质降级开关。刻意**不在**上面那个三段栅格内：那里的注释把「轨道数 ==
					内容数」写成了不变量，多塞一格就会留下一条装不满的轨道；而这一项是可配置
					的动作，与上面三个只读事实也不是同一类内容。

					`disabled={refreshing}` 是刷新那条守卫的反方向（见上）：刷新过程中拨动开关，
					随后回来的详情会把这次写入静默盖回旧值，用户看到开关「自己变回去了」。
				-->
				<QualityFallbackSetting
					value={source.quality_fallback}
					pending={sourcesController.savingQualityFallback}
					disabled={refreshing}
					onChange={handleQualityFallbackChange}
				/>

				<div class="space-y-3">
					<div>
						<h3 class="text-sm font-medium">脚本地址</h3>
						<p class="mt-1 text-xs leading-relaxed text-muted-foreground">
							将此 URL 导入洛雪音乐（LX Music）作为自定义音源。
						</p>
					</div>
					<CopyField value={source.script_url} />

					{#if renamedThisVisit}
						<!--
							warning 而非 danger：没有任何东西失败，只是用户要去别处做一个动作，
							与 sessionStatusTone 把「二维码过期」判成 warning 是同一条理由。
							用常驻 Alert 而非 toast：用户是在几分钟后、在另一个应用里执行这一步，
							而 toast 四秒就消失。锚在「脚本地址」块内，因为下一步要复制的就是它。
						-->
						<Alert tone="warning" title="需要重新导入才会在洛雪中生效">
							<p class="leading-relaxed">
								洛雪音乐在导入音源时就把脚本头部信息固定下来了，之后不会自动同步。要让它显示新标题，请删除该音源后重新导入上面的地址；地址本身没有变化，已导入的音源仍可正常播放。
							</p>
						</Alert>
					{/if}
				</div>

				{#if describeAccountError(source.account.last_error_code, source.account.last_error)}
					<div class="rounded-lg border border-danger/20 bg-danger-soft/60 p-4">
						<h3 class="text-xs font-bold tracking-wider text-danger uppercase">最后一次错误</h3>
						<p class="mt-1.5 text-sm leading-relaxed text-danger">
							{describeAccountError(source.account.last_error_code, source.account.last_error)}
						</p>
					</div>
				{/if}

				<div class="space-y-1">
					<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
						运行时 Token 预览
					</p>
					<p class="font-mono text-xs break-all text-muted-foreground/70">
						{source.runtime_token_preview}
					</p>
				</div>
			</CardContent>
		</Card>

		<LogsPanel {sourceId} />
	{/if}
</div>

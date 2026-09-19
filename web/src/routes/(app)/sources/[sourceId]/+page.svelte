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
	import LogsPanel from '$lib/components/source-logs/LogsPanel.svelte';
	import { sourcesController } from '$lib/features/sources/controller.svelte';
	import { toast } from '$lib/state/toast.svelte';
	import {
		accountStatusLabel,
		accountStatusTone,
		formatDateTime,
		labelOf,
		vipTone
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
						保存标题期间禁用刷新：刷新会 loadSource + loadSources 回填整页，
						把刚写入的标题静默盖回旧值——用户会看到标题「自己变回去了」。
						反方向的守卫（刷新中禁止改名）在 SourceTitle 内。
					-->
					<Button
						variant="outline"
						pending={refreshing}
						pendingLabel="刷新中…"
						disabled={sourcesController.savingName}
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
				-->
				<div class="grid grid-cols-2 gap-6 sm:grid-cols-3">
					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">状态</p>
						<StatusBadge tone={accountStatusTone(source.account.status)} dot>
							{labelOf(accountStatusLabel, source.account.status)}
						</StatusBadge>
					</div>

					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							VIP 状态
						</p>
						<StatusBadge tone={vipTone(source.account.vip_active)} dot>
							{source.account.vip_active ? '已激活' : '未激活'}
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

				{#if source.account.last_error}
					<div class="rounded-lg border border-danger/20 bg-danger-soft/60 p-4">
						<h3 class="text-xs font-bold tracking-wider text-danger uppercase">最后一次错误</h3>
						<p class="mt-1.5 text-sm leading-relaxed text-danger">
							{source.account.last_error}
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

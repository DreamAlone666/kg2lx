<script lang="ts">
	import { afterNavigate } from '$app/navigation';
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
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { sourcesController, type AccountAction } from '$lib/features/sources/controller.svelte';
	import { toast } from '$lib/state/toast.svelte';
	import {
		accountStatusLabel,
		accountStatusTone,
		describeAccountError,
		entitlementExpiringTone,
		formatDateTime,
		labelOf,
		summarizeEntitlements,
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
	const pendingAction = $derived(source && sourcesController.accountActions[source.account_id]);
	const snapshot = $derived(source ? summarizeEntitlements(source.account) : null);
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
	afterNavigate(({ to }) => {
		const id = to?.params?.sourceId;
		if (!id) return;
		renamedThisVisit = false;
		sourcesController.loadSource(id);
	});

	async function handleAccountAction(action: AccountAction) {
		if (!source) return;
		const result = await sourcesController.runAccountAction(source.source_id, source.account_id, action);
		if (result.status === 'ok') {
			toast.success(action === 'refresh' ? 'Token 已刷新。' : '权益已同步。');
		} else if (result.status === 'unconfirmed') {
			toast.error('操作已完成，请重新加载详情查看结果。');
		} else if (result.status === 'failed' && (action === 'refresh' || !source?.account.entitlements_sync_failed)) {
			toast.error(result.message);
		}
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

				</div>
			</CardHeader>

			<CardContent class="space-y-8 p-6">
				<section aria-labelledby="account-heading" class="space-y-3">
					<div class="flex flex-wrap items-center justify-between gap-3">
						<h2 id="account-heading" class="text-sm font-medium">账号登录</h2>
						<Button
							variant="outline"
							size="sm"
							pending={pendingAction === 'refresh'}
							pendingLabel="刷新中…"
							disabled={Boolean(pendingAction)}
							onclick={() => handleAccountAction('refresh')}
						>刷新 Token</Button>
					</div>
					<div class="grid grid-cols-2 gap-6">
						<div class="min-w-0 space-y-2">
							<p class="text-xs text-muted-foreground">登录状态</p>
							<StatusBadge tone={accountStatusTone(source.account.status)} dot>
								{labelOf(accountStatusLabel, source.account.status)}
							</StatusBadge>
						</div>
						<div class="min-w-0 space-y-2">
							<p class="text-xs text-muted-foreground">上次刷新 Token</p>
							<p class="tabular text-xs whitespace-nowrap sm:text-sm">
								{formatDateTime(source.account.last_refresh_at, '待刷新')}
							</p>
						</div>
					</div>
					{#if describeAccountError(source.account.last_error_code, source.account.last_error)}
						<Alert tone="danger" title="上次登录错误">
							{describeAccountError(source.account.last_error_code, source.account.last_error)}
						</Alert>
					{/if}
				</section>

				<section aria-labelledby="entitlements-heading" class="space-y-3 border-t border-border/70 pt-6">
					<div class="flex flex-wrap items-center justify-between gap-3">
						<h2 id="entitlements-heading" class="text-sm font-medium">会员权益</h2>
						<Button
							variant="outline"
							size="sm"
							pending={pendingAction === 'sync'}
							pendingLabel="同步中…"
							disabled={Boolean(pendingAction)}
							onclick={() => handleAccountAction('sync')}
						>同步权益</Button>
					</div>
					{#if snapshot?.state === 'failed'}
						<p class="text-sm text-danger">同步失败。请重试同步权益。</p>
					{:else if snapshot?.state === 'unknown'}
						<p class="text-sm text-muted-foreground">待同步。点击「同步权益」读取上游记录。</p>
					{/if}
					{#if snapshot && source.account.entitlements !== undefined}
						<p class="tabular text-xs text-muted-foreground">
							{source.account.entitlements_synced_at
								? `上次同步 ${formatDateTime(source.account.entitlements_synced_at)}`
								: '历史快照 · 同步时间未知'}
						</p>
						{#if snapshot.named.length > 0}
							<ul class="flex flex-wrap items-center gap-x-6 gap-y-2">
								{#each snapshot.named as entitlement, index (`${entitlement.product_type}:${index}`)}
									{@const expiringTone = entitlementExpiringTone(entitlement.end_at)}
									<li class="flex flex-wrap items-center gap-x-3 gap-y-1">
										<StatusBadge
											tone={VIP_ENTITLEMENT_TONE}
											emphasis={vipEntitlementEmphasis(entitlement.product_type)}
										>{labelOf(vipProductLabel, entitlement.product_type)}</StatusBadge>
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
						{#if snapshot.hasUnnamedActive}
							<p class="text-xs text-muted-foreground">上次记录还有未命名的在期权益。</p>
						{:else if snapshot.named.length === 0}
							<p class="text-sm text-muted-foreground">上次同步已确认无在期权益。</p>
						{/if}
					{/if}
				</section>

				<div class="border-t border-border/70 pt-8">
					<QualityFallbackSetting
						value={source.quality_fallback}
						pending={sourcesController.savingQualityFallback}
						onChange={handleQualityFallbackChange}
					/>
				</div>

				<!--
					【区段三：洛雪接入与凭证】
					将脚本地址与用于排查的运行时 Token 预览归纳在同一技术接入分区下，以分隔线与配置策略区隔。
				-->
				<div class="border-t border-border/70 pt-8 space-y-6">
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

					<div class="space-y-1.5">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							运行时 Token 预览
						</p>
						<div class="flex items-center gap-2">
							<Tooltip.Root>
								<Tooltip.Trigger type={null}>
									{#snippet child({ props })}
										<code
											class="inline-flex cursor-default items-center rounded-md border border-border bg-muted/60 px-2.5 py-1 font-mono text-xs text-muted-foreground transition-colors hover:border-foreground/20 hover:text-foreground"
											{...props}
										>
											{source.runtime_token_preview}
										</code>
									{/snippet}
								</Tooltip.Trigger>
								<Tooltip.Content>
									客户端取链鉴权凭据（出于安全仅展示前缀，完整 Token 已内置于上方脚本中）
								</Tooltip.Content>
							</Tooltip.Root>
						</div>
					</div>
				</div>
			</CardContent>
		</Card>

		{#key sourceId}
			<LogsPanel {sourceId} />
		{/key}
	{/if}
</div>

<script lang="ts">
	import { onDestroy } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import { afterNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
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
	import * as Tooltip from '$lib/components/ui/tooltip';
	import RenewalCard from '$lib/components/accounts/RenewalCard.svelte';
	import SourceNameEditor from '$lib/components/accounts/SourceNameEditor.svelte';
	import CopyField from '$lib/components/ui/CopyField.svelte';
	import QualityFallbackSetting from '$lib/components/accounts/QualityFallbackSetting.svelte';
	import { accountsController, type AccountAction } from '$lib/features/accounts/controller.svelte';
	import { adminSession, handleAuthError } from '$lib/state/admin-session.svelte';
	import { toast } from '$lib/state/toast.svelte';
	import type { StoredEvent } from '$lib/api/types';
	import {
		accountStatusLabel,
		accountStatusTone,
		describeAccountError,
		describeApiError,
		describeClaimResult,
		describeEventHeadline,
		eventOutcomeBadge,
		eventTypeLabel,
		formatDateTime,
		formatDateTimeMs,
		formatLatency,
		formatLogTimeMs,
		labelOf,
		providerLabel
	} from '$lib/domain';

	let accountId = $derived(page.params.accountId ?? '');
	let account = $derived(
		accountsController.currentAccount?.account_id === accountId
			? accountsController.currentAccount
			: (accountsController.accounts.find(item => item.account_id === accountId) ?? null)
	);
	const pendingAction = $derived(account ? accountsController.accountActions[account.account_id] : undefined);
	const savingAutoClaim = $derived(
		account ? Boolean(accountsController.savingAutoClaim[account.account_id]) : false
	);

	const primaryTitle = $derived(
		account
			? account.sources[0]?.name || `账号 ${account.userid}`
			: '账号详情'
	);
	const pageTitle = $derived(account ? `${primaryTitle} | kg2lx` : '账号详情 | kg2lx');

	const renamedSourceIds = new SvelteSet<string>();

	// 近期动态（底部通栏 6 条）
	let recentEvents = $state<StoredEvent[]>([]);
	let loadingRecent = $state(false);
	let recentError = $state<string | null>(null);
	let recentAbort: AbortController | null = null;

	async function loadRecentLogs(targetAccountId: string, silent = false) {
		const session = adminSession.signal;
		recentAbort?.abort();
		const request = new AbortController();
		recentAbort = request;

		if (!silent) loadingRecent = true;
		try {
			const res = await adminSession.api.listLogs(
				{ accountId: targetAccountId, limit: 6 },
				request.signal
			);
			if (session.aborted || request.signal.aborted || targetAccountId !== accountId) return;
			recentEvents = res.items;
			recentError = null;
		} catch (e: unknown) {
			if (session.aborted || request.signal.aborted || targetAccountId !== accountId) return;
			if (handleAuthError(e)) return;
			recentError = describeApiError(e);
		} finally {
			if (request === recentAbort) {
				recentAbort = null;
				loadingRecent = false;
			}
		}
	}

	afterNavigate(({ to }) => {
		const id = to?.params?.accountId ?? accountId;
		if (!id) return;
		renamedSourceIds.clear();
		recentEvents = [];
		recentError = null;
		void accountsController.loadAccount(id);
		void loadRecentLogs(id);
	});

	onDestroy(() => {
		recentAbort?.abort();
	});

	function markSourceRenamed(sourceId: string) {
		renamedSourceIds.add(sourceId);
	}

	async function handleAccountAction(action: AccountAction) {
		if (!account) return;
		const id = account.account_id;
		const result = await accountsController.runAccountAction(id, action);

		if (result.status === 'redirected' || result.status === 'busy') return;

		if (action === 'claim') {
			if (result.status === 'ok' && result.renewal) {
				const feedback = describeClaimResult(result.renewal);
				toast.push({ tone: feedback.tone, message: feedback.message });
			} else if (result.status === 'failed') {
				toast.error(result.message);
			} else if (result.status === 'unconfirmed') {
				toast.error('操作已发送，请刷新页面确认最新状态。');
			}
		} else {
			if (result.status === 'ok') {
				toast.success(action === 'refresh' ? 'Token 已刷新。' : '权益已同步。');
			} else if (result.status === 'unconfirmed') {
				toast.error('操作已完成，请重新加载详情查看结果。');
			} else if (result.status === 'failed') {
				toast.error(result.message);
			}
		}

		void loadRecentLogs(id, true);
	}

	async function handleToggleAutoClaim(next: boolean) {
		if (!account) return;
		const id = account.account_id;
		const res = await accountsController.setRenewalEnabled(id, next);
		if (res.status === 'ok') {
			toast.success(next ? '已开启自动领取 VIP。' : '已关闭自动领取 VIP。');
			void loadRecentLogs(id, true);
		} else if (res.status === 'failed') {
			toast.error(res.message);
		}
	}

	async function handleQualityFallbackChange(sourceId: string, next: boolean) {
		const res = await accountsController.setQualityFallback(sourceId, next);
		if (res.status === 'failed') toast.error(res.message);
	}
</script>

<svelte:head>
	<title>{pageTitle}</title>
</svelte:head>

<div class="space-y-6">
	{#if !account && accountsController.loadingDetail}
		<div class="space-y-4">
			<Skeleton class="h-4 w-32" />
			<Skeleton class="h-8 w-56" />
			<div class="grid gap-6 lg:grid-cols-12">
				<div class="space-y-6 lg:col-span-7">
					<Card class="p-6">
						<Skeleton class="h-6 w-40" />
						<Skeleton class="mt-4 h-16 w-full" />
						<Skeleton class="mt-4 h-36 w-full" />
					</Card>
				</div>
				<div class="space-y-6 lg:col-span-5">
					<Card class="p-6">
						<Skeleton class="h-6 w-32" />
						<Skeleton class="mt-4 h-11 w-full" />
					</Card>
				</div>
			</div>
		</div>
	{:else if !account}
		<Alert title="加载账号失败">
			<p class="leading-relaxed">
				{accountsController.error ?? '未找到该账号，可能已被移除。'}
			</p>
			<Button variant="outline" size="sm" class="mt-4" href="/accounts">返回账号列表</Button>
		</Alert>
	{:else}
		<!-- 轻量面包屑 -->
		<nav aria-label="面包屑" class="flex items-center gap-1.5 text-xs text-muted-foreground">
			<a href={resolve('/accounts')} class="hover:text-foreground hover:underline">账号</a>
			<span aria-hidden="true">/</span>
			<span class="truncate text-foreground">{primaryTitle}</span>
		</nav>

		<!-- 页头 -->
		<header
			class="flex flex-col gap-4 border-b border-border/70 pb-5 sm:flex-row sm:items-start sm:justify-between"
			style="view-transition-name: account-{account.account_id}"
		>
			<div class="min-w-0 flex-1">
				{#if account.sources.length === 1}
					{#key account.sources[0].source_id}
						<SourceNameEditor
							source={account.sources[0]}
							as="h1"
							onRenamed={markSourceRenamed}
						/>
					{/key}
				{:else}
					<h1 class="truncate text-2xl font-bold tracking-tight">{primaryTitle}</h1>
				{/if}
				<p class="mt-1 font-mono text-sm text-muted-foreground">
					账号 {account.userid} · {labelOf(providerLabel, account.provider)}
				</p>
			</div>

			{#if account.status !== 'active'}
				<div class="flex shrink-0 items-center">
					<StatusBadge tone={accountStatusTone(account.status)} dot>
						{labelOf(accountStatusLabel, account.status)}
					</StatusBadge>
				</div>
			{/if}
		</header>

		{#if accountsController.error}
			<Alert>
				<div class="flex items-start justify-between gap-4">
					<p class="leading-relaxed">{accountsController.error}</p>
					<Button variant="ghost" size="sm" onclick={() => accountsController.clearError()}>
						关闭
					</Button>
				</div>
			</Alert>
		{/if}

		<!-- 双栏工作台：左 7（账号域）右 5（洛雪音源域） -->
		<div class="grid gap-6 lg:grid-cols-12">
			<!-- 左栏：账号域 -->
			<div class="min-w-0 space-y-6 lg:col-span-7">
				<RenewalCard
					{account}
					{pendingAction}
					{savingAutoClaim}
					onSync={() => handleAccountAction('sync')}
					onClaim={() => handleAccountAction('claim')}
					onToggleAutoClaim={handleToggleAutoClaim}
					onDeadlineElapsed={() => {
						void accountsController.loadAccount(account.account_id, { silent: true });
						void loadRecentLogs(account.account_id, true);
					}}
				/>

				<Card>
					<CardHeader class="border-b border-border/70 pb-4">
						<div class="flex flex-wrap items-center justify-between gap-2">
							<h2 class="text-subheading">账号登录</h2>
							<div class="flex items-center gap-2">
								<Button variant="ghost" size="sm" href="/bind">重新扫码</Button>
								<Button
									variant="outline"
									size="sm"
									pending={pendingAction === 'refresh'}
									pendingLabel="刷新中…"
									disabled={Boolean(pendingAction)}
									onclick={() => handleAccountAction('refresh')}
								>
									刷新 Token
								</Button>
							</div>
						</div>
					</CardHeader>

					<CardContent class="space-y-4 p-6">
						<div class="grid grid-cols-2 gap-6">
							<div class="min-w-0 space-y-1.5">
								<p class="text-xs text-muted-foreground">登录状态</p>
								<StatusBadge tone={accountStatusTone(account.status)} dot>
									{labelOf(accountStatusLabel, account.status)}
								</StatusBadge>
							</div>
							<div class="min-w-0 space-y-1.5">
								<p class="text-xs text-muted-foreground">上次刷新 Token</p>
								<p class="tabular text-xs whitespace-nowrap sm:text-sm">
									{formatDateTime(account.last_refresh_at, '待刷新')}
								</p>
							</div>
						</div>

						{#if describeAccountError(account.last_error_code, account.last_error)}
							<Alert tone="danger" title="上次登录错误">
								{describeAccountError(account.last_error_code, account.last_error)}
							</Alert>
						{/if}
					</CardContent>
				</Card>
			</div>

			<!-- 右栏：洛雪音源域（遍历 account.sources 0..N） -->
			<div class="min-w-0 space-y-6 lg:col-span-5">
				{#if account.sources.length === 0}
					<Card>
						<CardHeader class="border-b border-border/70 pb-4">
							<h2 class="text-subheading">洛雪脚本接入</h2>
						</CardHeader>
						<CardContent class="p-6">
							<p class="text-sm leading-relaxed text-muted-foreground">
								当前账号下暂无关联音源。可通过重新扫码绑定自动创建默认音源。
							</p>
							<Button variant="outline" size="sm" class="mt-4" href="/bind">
								绑定账号
							</Button>
						</CardContent>
					</Card>
				{:else}
					{#each account.sources as source, index (source.source_id)}
						<div class="space-y-4">
							{#if account.sources.length > 1}
								<div class="rounded-xl bg-muted/50 px-4 py-3">
									<p class="mb-1 text-xs font-medium text-muted-foreground">
										音源 {index + 1}
									</p>
									{#key source.source_id}
										<SourceNameEditor
											{source}
											as="h2"
											onRenamed={markSourceRenamed}
										/>
									{/key}
								</div>
							{/if}

							<Card>
								<CardHeader class="border-b border-border/70 pb-4">
									<h2 class="text-subheading">洛雪脚本接入</h2>
								</CardHeader>
								<CardContent class="space-y-5 p-6">
									<div class="space-y-2.5">
										<div>
											<h3 class="text-sm font-medium">脚本地址</h3>
											<p class="mt-1 text-xs leading-relaxed text-muted-foreground">
												将此 URL 导入洛雪音乐（LX Music）作为自定义音源。
											</p>
										</div>
										<CopyField value={source.script_url} />

										{#if renamedSourceIds.has(source.source_id)}
											<Alert tone="warning" title="需要重新导入才会在洛雪中生效">
												<p class="leading-relaxed">
													洛雪音乐在导入音源时就把脚本头部信息固定下来了，之后不会自动同步。要让它显示新标题，请删除该音源后重新导入上面的地址；地址本身没有变化，已导入的音源仍可正常播放。
												</p>
											</Alert>
										{/if}
									</div>

									<div class="space-y-1.5 border-t border-border/70 pt-4">
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
								</CardContent>
							</Card>

							<Card>
								<CardHeader class="border-b border-border/70 pb-4">
									<h2 class="text-subheading">播放与降级策略</h2>
								</CardHeader>
								<CardContent class="p-6">
									<QualityFallbackSetting
										value={source.quality_fallback}
										pending={Boolean(accountsController.savingQualityFallback[source.source_id])}
										onChange={(next) => handleQualityFallbackChange(source.source_id, next)}
									/>
								</CardContent>
							</Card>
						</div>
					{/each}
				{/if}
			</div>
		</div>

		<!-- 底部通栏：近期动态 -->
		<Card>
			<CardHeader class="border-b border-border/70 pb-4">
				<div class="flex flex-wrap items-center justify-between gap-2">
					<h2 class="text-subheading">近期动态</h2>
					<Button
						variant="ghost"
						size="sm"
						href={`${resolve('/logs')}?account=${encodeURIComponent(account.account_id)}`}
					>
						查看完整日志 →
					</Button>
				</div>
			</CardHeader>
			<CardContent class="p-0">
				{#if loadingRecent && recentEvents.length === 0}
					<div class="space-y-2 p-4">
						{#each [0, 1, 2] as slot (slot)}
							<Skeleton class="h-10 w-full" />
						{/each}
					</div>
				{:else if recentError && recentEvents.length === 0}
					<div class="flex flex-wrap items-center justify-between gap-3 px-6 py-4 text-sm text-danger">
						<span>加载近期动态失败：{recentError}</span>
						<Button variant="outline" size="sm" onclick={() => loadRecentLogs(account.account_id)}>
							重试
						</Button>
					</div>
				{:else if recentEvents.length === 0}
					<div class="px-6 py-10 text-center text-sm text-muted-foreground">
						暂无近期动态，播放音乐或执行账号操作后会记录在此。
					</div>
				{:else}
					<ul class="divide-y divide-border/70">
						{#each recentEvents as event (event.log_id)}
							{@const headline = describeEventHeadline(event)}
							{@const badge = eventOutcomeBadge(event)}
							<li>
								<a
									href={`${resolve('/logs')}?account=${encodeURIComponent(account.account_id)}&selected=${encodeURIComponent(event.log_id)}`}
									class="flex flex-wrap items-center justify-between gap-3 px-6 py-3 transition-colors hover:bg-muted/40"
								>
									<div class="flex min-w-0 flex-1 items-center gap-3">
										<StatusBadge tone="neutral" class="shrink-0">
											{labelOf(eventTypeLabel, event.event_type)}
										</StatusBadge>
										<div class="min-w-0 flex-1">
											{#if headline.title !== labelOf(eventTypeLabel, event.event_type)}
												<p class="truncate text-sm font-medium text-foreground">{headline.title}</p>
											{/if}
											{#if headline.subtitle}
												<p class="truncate text-xs text-muted-foreground">
													{headline.subtitle}
												</p>
											{/if}
										</div>
									</div>

									<div class="flex shrink-0 items-center gap-3">
										<StatusBadge tone={badge.tone} class="font-mono">
											{badge.label}
										</StatusBadge>
										<span
											class="tabular font-mono text-xs text-muted-foreground"
											title={formatDateTimeMs(event.created_at_ms)}
										>
											{formatLogTimeMs(event.created_at_ms)}
										</span>
										<span class="tabular w-12 text-right font-mono text-xs text-muted-foreground">
											{event.latency_ms === null ? '—' : formatLatency(event.latency_ms)}
										</span>
									</div>
								</a>
							</li>
						{/each}
					</ul>
				{/if}
			</CardContent>
		</Card>
	{/if}
</div>

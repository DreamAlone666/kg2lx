<script lang="ts">
	import { onDestroy, onMount, untrack } from 'svelte';
	import { replaceState } from '$app/navigation';
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
	import LogInspector from '$lib/components/logs/LogInspector.svelte';
	import { logsController } from '$lib/features/logs/controller.svelte';
	import { usePageVisible } from '$lib/hooks/use-page-visible.svelte';
	import type { EventType, LogView, MusicUrlEvent, StoredEvent } from '$lib/api/types';
	import {
		describeEventHeadline,
		effectiveBitRate,
		eventDiagnosticErrorLabel,
		eventOutcomeBadge,
		eventTypeLabel,
		formatBitRate,
		formatClockTime,
		formatDateTime,
		formatLatency,
		formatLogTime,
		isEntitlementAlreadyDone,
		labelOf,
		qualityLabel,
		qualityTone
	} from '$lib/domain';
	import { EASE_OUT_SOFT } from '$lib/utils/motion';
	import { cn } from '$lib/utils.js';

	const pageVisible = usePageVisible();

	const typeTabs: { value: EventType | undefined; label: string }[] = [
		{ value: undefined, label: '全部' },
		{ value: 'music_url', label: '取链' },
		{ value: 'token_refresh', label: 'Token' },
		{ value: 'account_login', label: '登录' },
		{ value: 'entitlement', label: '权益' },
	];

	const validEventTypes = new Set<EventType>(['music_url', 'token_refresh', 'account_login', 'entitlement']);

	function parseUrlParams(searchParams: URLSearchParams) {
		const rawType = searchParams.get('type');
		const eventType = rawType && validEventTypes.has(rawType as EventType)
			? (rawType as EventType)
			: undefined;
		const view: LogView = searchParams.get('view') === 'errors' ? 'errors' : 'all';
		const accountId = searchParams.get('account') || undefined;
		const selectedLogId = searchParams.get('selected') || null;
		return { view, eventType, accountId, selectedLogId };
	}

	function pushUrlState(next: {
		view: LogView;
		eventType: EventType | undefined;
		accountId: string | undefined;
		selectedLogId: string | null;
	}) {
		if (typeof window === 'undefined') return;
		const url = new URL(window.location.href);
		if (next.eventType) url.searchParams.set('type', next.eventType);
		else url.searchParams.delete('type');

		if (next.accountId) url.searchParams.set('account', next.accountId);
		else url.searchParams.delete('account');

		if (next.view === 'errors') url.searchParams.set('view', 'errors');
		else url.searchParams.delete('view');

		if (next.selectedLogId) url.searchParams.set('selected', next.selectedLogId);
		else url.searchParams.delete('selected');

		replaceState(url, {});
	}

	onMount(() => {
		const initial = parseUrlParams(page.url.searchParams);
		logsController.start(initial);
	});

	onDestroy(() => {
		logsController.stop();
	});

	// 仅订阅 URL 查询串变化；用 untrack 隔离控制器内部读取，避免点击行改 selectedLogId 时
	// 在 replaceState 更新 page.url 之前立刻重跑本 effect 把选中状态覆写回旧 URL。
	$effect(() => {
		const search = page.url.search;
		const parsed = parseUrlParams(new URLSearchParams(search));
		untrack(() => logsController.syncFromUrl(parsed));
	});

	function handleFilterChange(patch: {
		view?: LogView;
		eventType?: EventType | undefined;
		accountId?: string | undefined;
	}) {
		const view = 'view' in patch && patch.view !== undefined ? patch.view : logsController.view;
		const eventType = 'eventType' in patch ? patch.eventType : logsController.eventType;
		const accountId = 'accountId' in patch ? patch.accountId : logsController.accountId;
		logsController.setFilters({ view, eventType, accountId });
		pushUrlState({
			view,
			eventType,
			accountId,
			selectedLogId: logsController.selectedLogId,
		});
	}

	function handleSelectEvent(logId: string | null) {
		logsController.selectEvent(logId);
		pushUrlState({
			view: logsController.view,
			eventType: logsController.eventType,
			accountId: logsController.accountId,
			selectedLogId: logId,
		});
	}

	const selectedEvent = $derived(logsController.selectedEvent);
	let hoveredCorrelationId = $state<string | null>(null);
	const correlationCounts = $derived.by(() => {
		const counts = new Map<string, number>();
		for (const item of logsController.logs) {
			counts.set(item.correlation_id, (counts.get(item.correlation_id) ?? 0) + 1);
		}
		return counts;
	});
	const activeCorrelationId = $derived.by(() => {
		if (selectedEvent) return selectedEvent.correlation_id;
		if (hoveredCorrelationId && (correlationCounts.get(hoveredCorrelationId) ?? 0) >= 2) {
			return hoveredCorrelationId;
		}
		return null;
	});
	const inspectorOpen = $derived(Boolean(selectedEvent));
	const showAccountColumn = $derived(!logsController.accountId && !inspectorOpen);
	const showOutcomeColumn = $derived(logsController.view === 'all');
	const showMusicColumns = $derived(
		logsController.logs.some((event) => event.event_type === 'music_url')
	);
	const showUpstreamColumn = $derived(
		!inspectorOpen &&
			logsController.logs.some(
				(event) =>
					event.event_type === 'music_url' &&
					(event.detail.upstream_status !== null || event.detail.upstream_priv_status !== null)
			)
	);
	const showLatencyColumn = $derived(!inspectorOpen);

	const cell = 'px-4 py-2.5 align-top';
	const heading = 'px-4 py-2.5 font-medium';
	const rowAnimation = `kg2lx-row-flash 900ms ${EASE_OUT_SOFT}`;

	function upstreamText(detail: MusicUrlEvent) {
		if (detail.upstream_status === null && detail.upstream_priv_status === null) return '—';
		if (detail.upstream_priv_status === null) return String(detail.upstream_status);
		return `${detail.upstream_status ?? '—'}/${detail.upstream_priv_status}`;
	}

	function upstreamTitle(detail: MusicUrlEvent) {
		const facts: string[] = [];
		if (detail.upstream_status !== null) facts.push(`status ${detail.upstream_status}`);
		if (detail.upstream_priv_status !== null) facts.push(`priv_status ${detail.upstream_priv_status}`);
		if (detail.upstream_fail_process !== null) {
			facts.push(`fail_process ${JSON.stringify(detail.upstream_fail_process)}`);
		}
		return facts.join(' · ') || '上游未提供观测值';
	}

	function actualQuality(detail: MusicUrlEvent) {
		const rate = effectiveBitRate(
			detail.upstream_bit_rate,
			detail.upstream_file_size,
			detail.upstream_time_length
		);
		const facts = [detail.upstream_ext_name, rate === null ? null : formatBitRate(rate)].filter(
			Boolean
		);
		return facts.length ? `实际 ${facts.join(' · ')}` : null;
	}

	function qualityTitle(detail: MusicUrlEvent) {
		const facts = [labelOf(qualityLabel, detail.requested_quality)];
		if (detail.attempted_quality && detail.attempted_quality !== detail.requested_quality) {
			facts.push(`尝试 ${detail.attempted_quality}`);
		}
		if (detail.upstream_quality !== null) facts.push(`上游参数 ${detail.upstream_quality}`);
		if (detail.upstream_file_size !== null) facts.push(`文件大小 ${detail.upstream_file_size} B`);
		if (detail.reported_types !== null) {
			facts.push(`客户端档位 ${JSON.stringify(detail.reported_types)}`);
		}
		return facts.join(' · ');
	}

	function rowHighlightClass(event: StoredEvent) {
		if (selectedEvent?.log_id === event.log_id) {
			return 'bg-primary-soft/50 hover:bg-primary-soft/60';
		}
		if (activeCorrelationId && event.correlation_id === activeCorrelationId) {
			return 'bg-primary-soft/25 hover:bg-primary-soft/40';
		}
		return 'hover:bg-muted/40';
	}
</script>

<svelte:head>
	<title>事件日志 | kg2lx</title>
</svelte:head>

{#snippet eventSummaryCell(event: StoredEvent)}
	{@const headline = describeEventHeadline(event)}
	{#if event.event_type === 'music_url'}
		<span class="block truncate font-medium text-foreground" title={headline.title}>
			{headline.title}
		</span>
		{#if headline.subtitle}
			<span class="block truncate text-muted-foreground" title={headline.subtitle}>
				{headline.subtitle}
			</span>
		{/if}
	{:else}
		<div class="flex flex-wrap items-baseline gap-x-2">
			<span class="font-medium text-foreground">{headline.title}</span>
			{#if headline.subtitle}
				<span class="text-muted-foreground">{headline.subtitle}</span>
			{/if}
		</div>
	{/if}
{/snippet}

{#snippet qualityCell(detail: MusicUrlEvent)}
	{@const actual = actualQuality(detail)}
	<div class="flex min-w-0 flex-col items-start gap-0.5">
		<StatusBadge
			tone={qualityTone(detail.requested_quality)}
			class="max-w-full truncate font-mono"
			title={qualityTitle(detail)}
		>
			{detail.requested_quality}
		</StatusBadge>
		{#if actual}
			<span class="w-full truncate text-caption text-muted-foreground" title={actual}>{actual}</span>
		{/if}
	</div>
{/snippet}

{#snippet diagnosticCell(event: StoredEvent)}
	{@const diagError = eventDiagnosticErrorLabel(event)}
	<div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
		{#if diagError}
			<StatusBadge tone="danger" title={event.error_code ?? undefined}>{diagError}</StatusBadge>
		{/if}
		{#if event.event_type === 'music_url'}
			{@const d = event.detail}
			{#if d.stage === 'precheck'}<span>预检</span>{/if}
			{#if d.quality_fallback_index > 0}
				<span class="font-medium text-warning" title={`本次请求档位 ${d.attempted_quality}`}>
					降级 {d.quality_fallback_index}
				</span>
			{/if}
			{#if d.retry_count > 0}<span class="font-medium text-warning">重试 {d.retry_count}</span>{/if}
			{#if !event.ok && d.status_code !== null}
				<span class="tabular font-mono">HTTP {d.status_code}</span>
			{/if}
		{:else if event.event_type === 'entitlement'}
			{@const d = event.detail}
			{#if isEntitlementAlreadyDone(d)}
				{#if d.upstream_code !== undefined && d.upstream_code !== null}
					<span class="tabular font-mono text-muted-foreground">幂等码 {d.upstream_code}</span>
				{/if}
			{:else if d.outcome === 'deferred'}
				<StatusBadge tone="info">已延后</StatusBadge>
			{:else if d.outcome === 'uncertain'}
				<StatusBadge tone="warning">结果待核对</StatusBadge>
			{/if}
			{#if d.http_status !== undefined && d.http_status !== null && !isEntitlementAlreadyDone(d) && !event.ok}
				<span class="tabular font-mono">HTTP {d.http_status}</span>
			{/if}
		{/if}
	</div>
{/snippet}

<div class="space-y-6">
	<header>
		<h1 class="text-heading tracking-tight">事件日志</h1>
		<p class="mt-1 text-sm text-muted-foreground">
			查看全站取链、Token 刷新、扫码登录与每日领升事件，点击任意记录可检视关联因果链路。
		</p>
	</header>

	<div class="lg:flex lg:items-start lg:gap-6">
		<Card class="min-w-0 flex-1 overflow-hidden">
			<CardHeader class="gap-3 border-b border-border/70">
				<!-- 一级分段切换 + 仅错误 + 实时控制 -->
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div
						class="flex flex-wrap items-center gap-1 rounded-lg bg-muted p-0.5"
						role="group"
						aria-label="事件类型筛选"
					>
						{#each typeTabs as tab (tab.label)}
							{@const active = logsController.eventType === tab.value}
							<button
								type="button"
								aria-pressed={active}
								class={cn(
									'touch-target cursor-pointer rounded-md px-3 text-xs font-medium transition-colors duration-150 ease-out-soft md:min-h-8',
									active
										? 'bg-background text-foreground shadow-soft'
										: 'text-muted-foreground hover:text-foreground'
								)}
								onclick={() => handleFilterChange({ eventType: tab.value })}
							>
								{tab.label}
							</button>
						{/each}
					</div>

					<div class="flex flex-wrap items-center gap-2">
						<button
							type="button"
							aria-pressed={logsController.view === 'errors'}
							class={cn(
								'touch-target cursor-pointer rounded-lg border px-3 text-xs font-medium transition-colors duration-150 ease-out-soft md:min-h-8',
								logsController.view === 'errors'
									? 'border-danger/30 bg-danger-soft text-danger'
									: 'border-border bg-background text-muted-foreground hover:text-foreground'
							)}
							onclick={() =>
								handleFilterChange({
									view: logsController.view === 'errors' ? 'all' : 'errors'
								})}
						>
							仅错误
						</button>

						{#if !pageVisible.visible || logsController.paused}
							<StatusBadge tone="neutral" dot>已暂停</StatusBadge>
						{:else if logsController.error}
							<StatusBadge tone="danger" dot>刷新失败</StatusBadge>
						{:else if logsController.loading}
							<StatusBadge tone="info" dot pulse>刷新中</StatusBadge>
						{:else}
							<StatusBadge tone="success" dot>实时</StatusBadge>
						{/if}

						{#if logsController.paused && logsController.lastUpdatedAt}
							<span class="tabular text-xs text-muted-foreground">
								数据截至 {formatClockTime(logsController.lastUpdatedAt)}
							</span>
						{/if}

						<Button
							variant="ghost"
							size="sm"
							onclick={() => logsController.setPaused(!logsController.paused)}
						>
							{logsController.paused ? '继续' : '暂停'}
						</Button>
						<Button
							variant="ghost"
							size="sm"
							pending={logsController.loading}
							pendingLabel="刷新中…"
							onclick={() => logsController.refresh()}
						>
							刷新
						</Button>
					</div>
				</div>

				<!-- 账号归属筛选条 -->
				{#if logsController.accounts.length > 0 || logsController.accountId}
					<div class="flex flex-wrap items-center gap-1.5 pt-1" role="group" aria-label="按账号筛选">
						<button
							type="button"
							aria-pressed={!logsController.accountId}
							class={cn(
								'cursor-pointer rounded-full border px-2.5 py-1 text-xs font-medium transition-colors',
								!logsController.accountId
									? 'border-primary/35 bg-primary-soft text-primary-soft-foreground'
									: 'border-border bg-background text-muted-foreground hover:text-foreground'
							)}
							onclick={() => handleFilterChange({ accountId: undefined })}
						>
							全部账号
						</button>

						{#each logsController.accounts as acc (acc.account_id)}
							{@const accLabel = acc.sources[0]?.name || acc.display_name || `账号 ${acc.userid}`}
							{@const active = logsController.accountId === acc.account_id}
							<button
								type="button"
								aria-pressed={active}
								class={cn(
									'max-w-44 cursor-pointer truncate rounded-full border px-2.5 py-1 text-xs font-medium transition-colors',
									active
										? 'border-primary/35 bg-primary-soft text-primary-soft-foreground'
										: 'border-border bg-background text-muted-foreground hover:text-foreground'
								)}
								title={`${accLabel} (账号 ${acc.userid})`}
								onclick={() => handleFilterChange({ accountId: acc.account_id })}
							>
								{accLabel}
							</button>
						{/each}
					</div>
				{/if}
			</CardHeader>

			<CardContent class="p-0">
				{#if logsController.error}
					<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border/70 px-4 py-2 text-sm text-danger">
						<span>加载日志失败。{logsController.error}</span>
						{#if logsController.logs.length > 0 && logsController.lastUpdatedAt}
							<span class="text-xs text-muted-foreground">
								以下为 {formatClockTime(logsController.lastUpdatedAt)} 的数据。
							</span>
						{/if}
						<Button variant="ghost" size="sm" onclick={() => logsController.refresh()}>重试</Button>
					</div>
				{/if}

				{#if logsController.logs.length > 0}
					<!-- 桌面端表格 -->
					<div class="hidden max-h-[42rem] overflow-auto md:block">
						<table class="w-full table-fixed text-left text-xs">
							<thead class="sticky top-0 z-10 bg-card shadow-[inset_0_-1px_0_0_var(--color-border)]">
								<tr>
									<th scope="col" class={heading}>事件</th>
									{#if showAccountColumn}<th scope="col" class={cn(heading, 'w-28 lg:w-32')}>账号</th>{/if}
									{#if showMusicColumns}<th scope="col" class={cn(heading, 'w-28 lg:w-36')}>音质</th>{/if}
									{#if showOutcomeColumn}<th scope="col" class={cn(heading, 'w-20')}>状态</th>{/if}
									{#if showUpstreamColumn}<th scope="col" class={cn(heading, 'w-16')}>上游</th>{/if}
									<th scope="col" class={cn(heading, 'w-28 lg:w-32')}>时间</th>
									{#if showLatencyColumn}<th scope="col" class={cn(heading, 'w-16')}>延迟</th>{/if}
									<th scope="col" class={cn(heading, inspectorOpen ? 'w-28 xl:w-36' : 'w-32 lg:w-44')}>诊断</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-border/70">
								{#each logsController.logs as event (event.log_id)}
									{@const badge = eventOutcomeBadge(event)}
									<tr
										class={cn('cursor-pointer transition-colors', rowHighlightClass(event))}
										style:animation={logsController.newLogIds.has(event.log_id) ? rowAnimation : undefined}
										onclick={() => handleSelectEvent(event.log_id)}
										onmouseenter={() => (hoveredCorrelationId = event.correlation_id)}
										onmouseleave={() => {
											if (hoveredCorrelationId === event.correlation_id) hoveredCorrelationId = null;
										}}
									>
										<td class={cell}>
											<button
												type="button"
												class="w-full cursor-pointer rounded-sm text-left"
												onclick={(e) => {
													e.stopPropagation();
													handleSelectEvent(event.log_id);
												}}
											>
												{@render eventSummaryCell(event)}
											</button>
										</td>
										{#if showAccountColumn}
											<td class={cn(cell, 'truncate text-muted-foreground')}>
												<span title={logsController.accountDisplayName(event.account_id)}>
													{logsController.accountDisplayName(event.account_id)}
												</span>
											</td>
										{/if}
										{#if showMusicColumns}
											<td class={cell}>
												{#if event.event_type === 'music_url'}
													{@render qualityCell(event.detail)}
												{/if}
											</td>
										{/if}
										{#if showOutcomeColumn}
											<td class={cell}>
												<StatusBadge tone={badge.tone} class="font-mono">
													{badge.label}
												</StatusBadge>
											</td>
										{/if}
										{#if showUpstreamColumn}
											<td class={cn(cell, 'tabular font-mono')}>
												{#if event.event_type === 'music_url'}
													<span title={upstreamTitle(event.detail)}>
														{upstreamText(event.detail)}
													</span>
												{/if}
											</td>
										{/if}
										<td
											class={cn(cell, 'tabular whitespace-nowrap font-mono')}
											title={formatDateTime(event.created_at)}
										>
											{formatLogTime(event.created_at)}
										</td>
										{#if showLatencyColumn}
											<td class={cn(cell, 'tabular whitespace-nowrap font-mono')}>
												{formatLatency(event.latency_ms)}
											</td>
										{/if}
										<td class={cell}>
											{@render diagnosticCell(event)}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>

					<!-- 移动端列表 -->
					<ul class="divide-y divide-border/70 md:hidden">
						{#each logsController.logs as event (event.log_id)}
							{@const badge = eventOutcomeBadge(event)}
							<li
								class={cn('px-4 py-3 transition-colors', rowHighlightClass(event))}
								style:animation={logsController.newLogIds.has(event.log_id) ? rowAnimation : undefined}
							>
								<button
									type="button"
									class="w-full space-y-2 text-left"
									onclick={() => handleSelectEvent(event.log_id)}
								>
									<div class="flex items-start justify-between gap-2">
										<div class="min-w-0 flex-1 text-xs">
											{@render eventSummaryCell(event)}
										</div>
										{#if showOutcomeColumn}
											<StatusBadge tone={badge.tone} class="shrink-0 font-mono">
												{badge.label}
											</StatusBadge>
										{/if}
									</div>

									<div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
										{#if showAccountColumn}
											<span class="truncate">{logsController.accountDisplayName(event.account_id)}</span>
										{/if}
										{#if event.event_type === 'music_url'}
											{@render qualityCell(event.detail)}
										{/if}
										<span class="tabular font-mono" title={formatDateTime(event.created_at)}>
											{formatLogTime(event.created_at)}
										</span>
										{#if event.latency_ms !== null}
											<span class="tabular font-mono">{formatLatency(event.latency_ms)}</span>
										{/if}
									</div>

									{@render diagnosticCell(event)}
								</button>
							</li>
						{/each}
					</ul>

					{#if logsController.hasMore}
						<div class="flex justify-center border-t border-border/70 p-4">
							<Button
								variant="outline"
								size="sm"
								pending={logsController.loadingMore}
								pendingLabel="加载中…"
								onclick={() => logsController.loadMore()}
							>
								加载更早记录
							</Button>
						</div>
					{/if}
				{:else if logsController.loading}
					<div class="space-y-2 p-4">
						{#each [0, 1, 2, 3, 4] as slot (slot)}
							<Skeleton class="h-9 w-full" />
						{/each}
					</div>
				{:else if !logsController.error}
					<div class="px-6 py-14 text-center text-sm text-muted-foreground">
						<p>
							{logsController.view === 'errors' ? '暂无符合条件的错误记录。' : '暂无符合条件的事件记录。'}
						</p>
						{#if logsController.eventType || logsController.accountId || logsController.view === 'errors'}
							<Button
								variant="outline"
								size="sm"
								class="mt-4"
								onclick={() =>
									handleFilterChange({ view: 'all', eventType: undefined, accountId: undefined })}
							>
								重置筛选条件
							</Button>
						{/if}
					</div>
				{/if}
			</CardContent>
		</Card>

		<LogInspector
			event={selectedEvent}
			correlationLogs={logsController.correlationLogs}
			loadingCorrelation={logsController.loadingCorrelation}
			correlationError={logsController.correlationError}
			onSelect={(logId) => handleSelectEvent(logId)}
			onRetryCorrelation={() => {
				if (selectedEvent) void logsController.loadCorrelation(selectedEvent.correlation_id, true);
			}}
			onClose={() => handleSelectEvent(null)}
		/>
	</div>
</div>

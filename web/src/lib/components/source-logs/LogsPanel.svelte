<script lang="ts">
	import { onMount } from 'svelte';
	import { Button, Card, CardContent, CardHeader, Skeleton, StatusBadge } from '$lib/components/ui';
	import { sourceLogsController } from '$lib/features/source-logs/controller.svelte';
	import { usePageVisible } from '$lib/hooks/use-page-visible.svelte';
	import type { AccountLoginEvent, EventType, MusicUrlEvent, SourceEvent, TokenRefreshEvent } from '$lib/api/types';
	import {
		effectiveBitRate,
		errorCodeLabel,
		formatBitRate,
		formatClockTime,
		formatDateTime,
		formatLatency,
		formatLogTime,
		labelOf,
		qualityLabel,
		qualityTone,
		requestOutcomeTone,
		shortId
	} from '$lib/domain';
	import { EASE_OUT_SOFT } from '$lib/utils/motion';
	import { cn } from '$lib/utils.js';

	let { sourceId }: { sourceId: string } = $props();
	const controller = sourceLogsController;
	const pageVisible = usePageVisible();

	const eventLabels: Record<EventType, string> = {
		music_url: '取链',
		token_refresh: 'Token 刷新',
		account_login: '账号登录',
		entitlement: '权益同步'
	};
	const refreshReasons: Record<TokenRefreshEvent['reason'], string> = {
		ttl: '登录态需要刷新',
		rejected: '上游鉴权拒绝',
		device: '设备注册',
		manual: '手动刷新',
		qr_login: '扫码授权'
	};
	const loginOutcomes: Record<AccountLoginEvent['outcome'], string> = {
		bound: '绑定成功',
		rebound: '重新绑定',
		expired: '二维码过期',
		failed: '登录失败'
	};

	const showOutcome = $derived(controller.view === 'all');
	const showMusicColumns = $derived(controller.logs.some((event) => event.event_type === 'music_url'));
	const showUpstream = $derived(controller.logs.some(
		(event) => event.event_type === 'music_url' && (
			event.detail.upstream_status !== null || event.detail.upstream_priv_status !== null
		)
	));
	const cell = 'px-4 py-2.5';
	const heading = `${cell} font-medium`;
	const rowAnimation = `kg2lx-row-flash 900ms ${EASE_OUT_SOFT}`;

	onMount(() => {
		controller.startPolling(sourceId);
		return () => controller.stopPolling();
	});

	function latencyText(event: SourceEvent) {
		return event.latency_ms === null ? '—' : formatLatency(event.latency_ms);
	}

	function eventDescription(event: Exclude<SourceEvent, { event_type: 'music_url' }>) {
		switch (event.event_type) {
			case 'token_refresh':
				return refreshReasons[event.detail.reason];
			case 'account_login':
				return loginOutcomes[event.detail.outcome];
			case 'entitlement':
				return event.ok ? '已更新上游快照' : '读取上游快照失败';
		}
	}

	function upstreamText(detail: MusicUrlEvent) {
		if (detail.upstream_status === null && detail.upstream_priv_status === null) return '—';
		if (detail.upstream_priv_status === null) return String(detail.upstream_status);
		return `${detail.upstream_status ?? '—'}/${detail.upstream_priv_status}`;
	}

	function upstreamTitle(detail: MusicUrlEvent) {
		const facts = [];
		if (detail.upstream_status !== null) facts.push(`status ${detail.upstream_status}`);
		if (detail.upstream_priv_status !== null) facts.push(`priv_status ${detail.upstream_priv_status}`);
		if (detail.upstream_fail_process !== null) facts.push(`fail_process ${JSON.stringify(detail.upstream_fail_process)}`);
		return facts.join(' · ') || '上游未提供观测值';
	}

	function actualQuality(detail: MusicUrlEvent) {
		const rate = effectiveBitRate(detail.upstream_bit_rate, detail.upstream_file_size, detail.upstream_time_length);
		const facts = [detail.upstream_ext_name, rate === null ? null : formatBitRate(rate)].filter(Boolean);
		return facts.length ? `实际 ${facts.join(' · ')}` : null;
	}

	function qualityTitle(detail: MusicUrlEvent) {
		const facts = [labelOf(qualityLabel, detail.requested_quality)];
		if (detail.attempted_quality && detail.attempted_quality !== detail.requested_quality) {
			facts.push(`尝试 ${detail.attempted_quality}`);
		}
		if (detail.upstream_quality !== null) facts.push(`上游参数 ${detail.upstream_quality}`);
		if (detail.upstream_file_size !== null) facts.push(`文件大小 ${detail.upstream_file_size} B`);
		if (detail.reported_types !== null) facts.push(`客户端档位 ${JSON.stringify(detail.reported_types)}`);
		return facts.join(' · ');
	}

	function errorLabel(event: SourceEvent) {
		if (event.event_type === 'token_refresh' || event.event_type === 'music_url') {
			if (event.detail.human_verification) return '上游需要人机验证';
		}
		return labelOf(errorCodeLabel, event.error_code, '操作失败');
	}

	function errorTitle(event: SourceEvent) {
		const facts = event.error_code ? [event.error_code] : [];
		if (event.event_type === 'music_url') {
			if (event.detail.upstream_endpoint) facts.push(event.detail.upstream_endpoint);
			facts.push(upstreamTitle(event.detail));
		} else if (event.event_type === 'token_refresh') {
			facts.push(event.detail.upstream_endpoint);
		}
		return facts.join(' · ');
	}
</script>

{#snippet song(detail: MusicUrlEvent)}
	{#if detail.track_title}
		<span class="block truncate font-medium text-foreground" title={detail.track_title}>{detail.track_title}</span>
		<span class="block truncate text-muted-foreground">
			{detail.artist_name || '未知歌手'}{#if detail.album_name} · {detail.album_name}{/if}
		</span>
	{:else}
		<span class="block truncate font-mono" title={`歌曲 hash ${detail.request_hash}`}>#{shortId(detail.request_hash)}</span>
		{#if detail.album_audio_id}
			<span class="block truncate font-mono text-caption text-muted-foreground" title={`album_audio_id ${detail.album_audio_id}`}>
				{detail.album_audio_id}
			</span>
		{/if}
	{/if}
{/snippet}

{#snippet quality(detail: MusicUrlEvent)}
	{@const actual = actualQuality(detail)}
	<div class="flex min-w-0 flex-col items-start gap-0.5">
		<StatusBadge tone={qualityTone(detail.requested_quality)} class="max-w-full truncate font-mono" title={qualityTitle(detail)}>
			{detail.requested_quality}
		</StatusBadge>
		{#if actual}<span class="w-full truncate text-caption text-muted-foreground" title={actual}>{actual}</span>{/if}
	</div>
{/snippet}

{#snippet outcome(event: SourceEvent)}
	<StatusBadge tone={requestOutcomeTone(event.ok)} class="font-mono">
		{event.ok ? 'OK' : 'ERR'}
	</StatusBadge>
{/snippet}

{#snippet diagnostic(event: SourceEvent)}
	<div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-muted-foreground">
		{#if !event.ok}
			<StatusBadge tone="danger" title={errorTitle(event)}>{errorLabel(event)}</StatusBadge>
		{/if}
		{#if event.event_type === 'music_url'}
			{@const detail = event.detail}
			{#if detail.stage === 'precheck'}<span>预检</span>{/if}
			{#if detail.quality_fallback_index > 0}
				<span class="font-medium text-warning" title={`本次请求档位 ${detail.attempted_quality}`}>降级 {detail.quality_fallback_index}</span>
			{/if}
			{#if detail.retry_count > 0}<span class="font-medium text-warning">重试 {detail.retry_count}</span>{/if}
			{#if !event.ok && detail.status_code !== null}<span class="tabular font-mono">HTTP {detail.status_code}</span>{/if}
		{/if}
		<span class="font-mono" title={`请求关联 ID ${event.correlation_id}`}>#{event.correlation_id.slice(-7)}</span>
	</div>
{/snippet}

<Card class="mt-6 overflow-hidden">
	<CardHeader class="gap-0 border-b border-border/70">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
				<h2 class="text-subheading" id="runtime-logs-heading">kg2lx 事件日志</h2>
				{#if !pageVisible.visible || controller.paused}
					<StatusBadge tone="neutral" dot>已暂停</StatusBadge>
				{:else if controller.error}
					<StatusBadge tone="danger" dot>刷新失败</StatusBadge>
				{:else if controller.loading}
					<StatusBadge tone="info" dot pulse>刷新中</StatusBadge>
				{:else}
					<StatusBadge tone="success" dot>实时</StatusBadge>
				{/if}
				{#if controller.paused && controller.lastUpdatedAt}
					<span class="tabular text-xs text-muted-foreground">数据截至 {formatClockTime(controller.lastUpdatedAt)}</span>
				{/if}
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<div class="flex rounded-lg bg-muted px-0.5 md:p-0.5" role="group" aria-label="日志结果">
					{#each ['all', 'errors'] as view (view)}
						{@const active = controller.view === view}
						<button
							type="button"
							aria-pressed={active}
							class={cn(
								'min-h-11 cursor-pointer rounded-md px-3.5 text-xs font-medium transition-colors duration-150 ease-out-soft md:min-h-8',
								active ? 'bg-background text-foreground shadow-soft' : 'text-muted-foreground hover:text-foreground'
							)}
							onclick={() => controller.setFilters(view === 'errors' ? 'errors' : 'all', controller.eventType)}
						>{view === 'errors' ? '仅错误' : '全部'}</button>
					{/each}
				</div>
				<label>
					<span class="sr-only">事件类别</span>
					<select
						class="min-h-11 rounded-md border border-border bg-card px-2.5 text-xs text-foreground md:min-h-9"
						value={controller.eventType ?? ''}
						onchange={(event) => {
							const type = event.currentTarget.value as EventType | '';
							controller.setFilters(controller.view, type || undefined);
						}}
					>
						<option value="">全部事件</option>
						{#each Object.entries(eventLabels) as [type, label] (type)}
							<option value={type}>{label}</option>
						{/each}
					</select>
				</label>
				<Button variant="ghost" size="sm" onclick={() => controller.setPaused(!controller.paused)}>
					{controller.paused ? '继续' : '暂停'}
				</Button>
				<Button variant="ghost" size="sm" pending={controller.loading} pendingLabel="刷新中…" onclick={() => controller.refresh()}>
					刷新
				</Button>
			</div>
		</div>
	</CardHeader>

	<CardContent class="p-0">
		{#if controller.error}
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border/70 px-4 py-2 text-sm text-danger">
				<span>加载日志失败。{controller.error}</span>
				{#if controller.logs.length > 0 && controller.lastUpdatedAt}
					<span class="text-xs text-muted-foreground">以下为 {formatClockTime(controller.lastUpdatedAt)} 的数据。</span>
				{/if}
				<Button variant="ghost" size="sm" onclick={() => controller.refresh()}>重试</Button>
			</div>
		{/if}
		{#if controller.logs.length > 0}
			<div class="hidden max-h-[36rem] overflow-auto md:block">
				<table class="w-full table-fixed text-left text-xs" aria-labelledby="runtime-logs-heading">
					<thead class="sticky top-0 bg-card">
						<tr class="border-b border-border/70">
							<th scope="col" class={heading}>事件</th>
							{#if showMusicColumns}<th scope="col" class={cn(heading, 'w-28 lg:w-40')}>音质</th>{/if}
							{#if showOutcome}<th scope="col" class={cn(heading, 'w-16')}>状态</th>{/if}
							{#if showUpstream}<th scope="col" class={cn(heading, 'w-16')}>上游</th>{/if}
							<th scope="col" class={cn(heading, 'w-28 lg:w-36')}>时间</th>
							<th scope="col" class={cn(heading, 'w-20')}>延迟</th>
							<th scope="col" class={cn(heading, 'w-40 lg:w-60')}>诊断</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-border/70">
						{#each controller.logs as event (event.log_id)}
							<tr
								data-event-type={event.event_type}
								style:animation={controller.newLogIds.has(event.log_id) ? rowAnimation : undefined}
							>
								{#if event.event_type === 'music_url'}
									<td class={cell}>{@render song(event.detail)}</td>
									<td class={cell}>{@render quality(event.detail)}</td>
								{:else}
									<td class={cell} colspan={showMusicColumns ? 2 : 1}>
										<span class="font-medium">{eventLabels[event.event_type]}</span>
										<span class="ml-2 text-muted-foreground">{eventDescription(event)}</span>
									</td>
								{/if}
								{#if showOutcome}<td class={cell}>{@render outcome(event)}</td>{/if}
								{#if showUpstream}
									<td class={cn(cell, 'tabular font-mono')}>
										{#if event.event_type === 'music_url'}
											<span title={upstreamTitle(event.detail)}>{upstreamText(event.detail)}</span>
										{/if}
									</td>
								{/if}
								<td class={cn(cell, 'tabular whitespace-nowrap font-mono')} title={formatDateTime(event.created_at)}>{formatLogTime(event.created_at)}</td>
								<td class={cn(cell, 'tabular whitespace-nowrap font-mono')}>{latencyText(event)}</td>
								<td class={cell}>{@render diagnostic(event)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>

			<ul class="divide-y divide-border/70 md:hidden" aria-labelledby="runtime-logs-heading">
				{#each controller.logs as event (event.log_id)}
					<li class="space-y-2 px-4 py-3" data-event-type={event.event_type}
						style:animation={controller.newLogIds.has(event.log_id) ? rowAnimation : undefined}>
						<div class="flex items-start justify-between gap-2">
							<div class="min-w-0 flex-1 text-xs">
								{#if event.event_type === 'music_url'}
									{@render song(event.detail)}
								{:else}
									<span class="font-medium">{eventLabels[event.event_type]}</span>
									<span class="ml-2 text-muted-foreground">{eventDescription(event)}</span>
								{/if}
							</div>
							{#if showOutcome}{@render outcome(event)}{/if}
						</div>
						<div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
							{#if event.event_type === 'music_url'}
								{@render quality(event.detail)}
								{#if showUpstream}
									<span class="tabular font-mono" title={upstreamTitle(event.detail)}>{upstreamText(event.detail)}</span>
								{/if}
							{/if}
							<span class="tabular font-mono" title={formatDateTime(event.created_at)}>{formatLogTime(event.created_at)}</span>
							{#if event.latency_ms !== null}<span class="tabular font-mono">{latencyText(event)}</span>{/if}
						</div>
						{@render diagnostic(event)}
					</li>
				{/each}
			</ul>
		{:else if controller.loading}
			<div class="space-y-2 p-4">
				{#each [0, 1, 2, 3] as slot (slot)}<Skeleton class="h-9 w-full" />{/each}
			</div>
		{:else if !controller.error}
			<div class="px-6 py-14 text-center text-sm text-muted-foreground">
				{controller.view === 'errors' ? '暂无符合条件的错误记录。' : '暂无符合条件的事件记录。'}
			</div>
		{/if}
	</CardContent>
</Card>

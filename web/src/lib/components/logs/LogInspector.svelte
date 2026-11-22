<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { Alert, Button, Card, CardContent, CardHeader, Skeleton, StatusBadge } from '$lib/components/ui';
	import * as Dialog from '$lib/components/ui/dialog';
	import type { StoredEvent } from '$lib/api/types';
	import { logsController } from '$lib/features/logs/controller.svelte';
	import {
		accountLoginOutcomeLabel,
		describeEventHeadline,
		effectiveBitRate,
		entitlementActionLabel,
		errorCodeLabel,
		eventDiagnosticErrorLabel,
		eventOutcomeBadge,
		eventTriggerLabel,
		eventTypeLabel,
		formatBitRate,
		formatClockTime,
		formatDateTime,
		formatLatency,
		isEntitlementAlreadyDone,
		isEventSuccess,
		labelOf,
		qualityLabel,
		renewalOutcomeLabel,
		renewalReasonLabel,
		runtimeStageLabel,
		tokenRefreshReasonLabel
	} from '$lib/domain';
	import { toneDotClass } from '$lib/components/ui/tone-classes';
	import { toast } from '$lib/state/toast.svelte';
	import { writeToClipboard } from '$lib/utils/clipboard';
	import { cn } from '$lib/utils.js';

	/**
	 * Master-Detail 事件详情检查器：
	 *   - 桌面端（lg+）：右侧 sticky 分栏展开，不遮挡左侧表格，支持 Escape 关闭
	 *   - 移动端（< lg）：通过 bits-ui Dialog（Portal + 焦点陷阱 + Escape）弹出详情
	 */
	let {
		event,
		correlationLogs,
		loadingCorrelation,
		correlationError,
		onSelect,
		onRetryCorrelation,
		onClose
	}: {
		event: StoredEvent | null;
		correlationLogs: StoredEvent[];
		loadingCorrelation: boolean;
		correlationError: string | null;
		onSelect: (logId: string) => void;
		onRetryCorrelation: () => void;
		onClose: () => void;
	} = $props();

	let isDesktop = $state(
		typeof window !== 'undefined' ? window.matchMedia('(min-width: 1024px)').matches : true
	);

	onMount(() => {
		const mql = window.matchMedia('(min-width: 1024px)');
		isDesktop = mql.matches;
		const onChange = (e: MediaQueryListEvent) => {
			isDesktop = e.matches;
		};
		mql.addEventListener('change', onChange);

		const onKeydown = (e: KeyboardEvent) => {
			if (e.key === 'Escape' && isDesktop && event) {
				onClose();
			}
		};
		window.addEventListener('keydown', onKeydown);

		return () => {
			mql.removeEventListener('change', onChange);
			window.removeEventListener('keydown', onKeydown);
			if (copyTimer) clearTimeout(copyTimer);
		};
	});

	let copiedField = $state<string | null>(null);
	let copyTimer: ReturnType<typeof setTimeout> | null = null;

	async function copyText(fieldKey: string, text: string, label: string) {
		const ok = await writeToClipboard(text);
		if (ok) {
			copiedField = fieldKey;
			toast.success(`已复制${label}。`);
			if (copyTimer) clearTimeout(copyTimer);
			copyTimer = setTimeout(() => {
				copiedField = null;
			}, 2000);
		} else {
			toast.error(`复制${label}失败。`);
		}
	}

	function formatReportedTypes(raw: unknown): string | null {
		if (raw === null || raw === undefined) return null;
		try {
			return JSON.stringify(raw, null, 2);
		} catch {
			return String(raw);
		}
	}
</script>

{#snippet inspectorBody(ev: StoredEvent)}
	{@const badge = eventOutcomeBadge(ev)}
	{@const diagError = eventDiagnosticErrorLabel(ev)}
	{@const headline = describeEventHeadline(ev)}
	{@const accountLabel = logsController.accountDisplayName(ev.account_id)}
	{@const sourceName = logsController.sourceDisplayName(ev.account_id, ev.source_id)}

	<div class="space-y-5 text-xs">
		{#if diagError}
			<Alert tone="danger" title={diagError}>
				{#if ev.error_code}
					<p class="font-mono text-xs">{ev.error_code}</p>
				{/if}
			</Alert>
		{/if}

		<!-- 基础事实 -->
		<section class="space-y-2.5">
			<h3 class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">事件事实</h3>
			<dl class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
				<dt class="text-muted-foreground">摘要</dt>
				<dd class="font-medium text-foreground">
					{headline.title}{headline.subtitle ? ` · ${headline.subtitle}` : ''}
				</dd>

				<dt class="text-muted-foreground">时间</dt>
				<dd class="tabular font-mono">{formatDateTime(ev.created_at)}</dd>

				<dt class="text-muted-foreground">耗时 / 序号</dt>
				<dd class="tabular font-mono">
					{formatLatency(ev.latency_ms)} · seq #{ev.seq}
				</dd>

				<dt class="text-muted-foreground">触发来源</dt>
				<dd>{labelOf(eventTriggerLabel, ev.trigger)}</dd>

				<dt class="text-muted-foreground">归属账号</dt>
				<dd class="min-w-0">
					{#if ev.account_id}
						<a
							href={resolve(`/accounts/${ev.account_id}`)}
							class="font-medium text-primary hover:underline"
						>
							{accountLabel}
						</a>
					{:else}
						<span class="text-muted-foreground">扫码登录（无归属账号）</span>
					{/if}
				</dd>

				{#if ev.source_id}
					<dt class="text-muted-foreground">关联音源</dt>
					<dd class="min-w-0 break-all">
						{sourceName && sourceName !== ev.source_id ? `${sourceName} ` : ''}
						<span class="font-mono text-muted-foreground">({ev.source_id})</span>
					</dd>
				{/if}

				<dt class="self-center text-muted-foreground">关联 ID</dt>
				<dd class="flex min-w-0 items-center gap-2">
					<code class="min-w-0 flex-1 truncate font-mono text-foreground">{ev.correlation_id}</code>
					<Button
						variant="ghost"
						size="sm"
						class="h-7 shrink-0 px-2 text-xs md:h-7"
						onclick={() => copyText('corr', ev.correlation_id, '关联 ID')}
					>
						{copiedField === 'corr' ? '已复制' : '复制'}
					</Button>
				</dd>

				{#if ev.error_code && isEventSuccess(ev)}
					<dt class="text-muted-foreground">参考码</dt>
					<dd class="font-mono text-muted-foreground">
						{labelOf(errorCodeLabel, ev.error_code, ev.error_code)} ({ev.error_code})
					</dd>
				{/if}
			</dl>
		</section>

		<!-- 事件专属字段 -->
		<section class="space-y-2.5 border-t border-border/70 pt-4">
			<h3 class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
				{labelOf(eventTypeLabel, ev.event_type)}详情
			</h3>

			{#if ev.event_type === 'music_url'}
				{@const d = ev.detail}
				{@const rate = effectiveBitRate(d.upstream_bit_rate, d.upstream_file_size, d.upstream_time_length)}
				{@const reportedJson = formatReportedTypes(d.reported_types)}
				<dl class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
					<dt class="text-muted-foreground">阶段</dt>
					<dd>{labelOf(runtimeStageLabel, d.stage)}</dd>

					{#if d.track_title}
						<dt class="text-muted-foreground">曲目</dt>
						<dd class="break-words">{d.track_title}</dd>
					{/if}
					{#if d.artist_name || d.album_name}
						<dt class="text-muted-foreground">歌手 / 专辑</dt>
						<dd class="break-words">
							{d.artist_name || '未知歌手'}{d.album_name ? ` · ${d.album_name}` : ''}
						</dd>
					{/if}

					<dt class="text-muted-foreground">请求 Hash</dt>
					<dd class="space-y-1">
						<code class="block break-all rounded-md bg-muted/60 px-2 py-1 font-mono text-xs text-foreground">
							{d.request_hash}
						</code>
						<Button
							variant="outline"
							size="sm"
							class="h-7 px-2.5 text-xs md:h-7"
							onclick={() => copyText('hash', d.request_hash, '32 位歌曲 Hash')}
						>
							{copiedField === 'hash' ? '已复制 Hash' : '复制完整 Hash'}
						</Button>
					</dd>

					{#if d.album_audio_id}
						<dt class="text-muted-foreground">Audio ID</dt>
						<dd class="font-mono break-all">{d.album_audio_id}</dd>
					{/if}

					<dt class="text-muted-foreground">请求音质</dt>
					<dd class="font-mono">
						{d.requested_quality} ({labelOf(qualityLabel, d.requested_quality)})
					</dd>

					{#if d.attempted_quality}
						<dt class="text-muted-foreground">尝试音质</dt>
						<dd class="font-mono">
							{d.attempted_quality}
							{d.quality_fallback_index > 0 ? `（降级 #${d.quality_fallback_index}）` : ''}
						</dd>
					{/if}

					{#if d.retry_count > 0}
						<dt class="text-muted-foreground">重试次数</dt>
						<dd class="tabular font-mono">{d.retry_count}</dd>
					{/if}

					{#if d.upstream_ext_name || rate !== null || d.upstream_file_size !== null}
						<dt class="text-muted-foreground">实际产物</dt>
						<dd class="tabular font-mono">
							{[
								d.upstream_ext_name,
								rate !== null ? formatBitRate(rate) : null,
								d.upstream_file_size !== null ? `${d.upstream_file_size} B` : null,
								d.upstream_time_length !== null ? `${d.upstream_time_length}s` : null
							]
								.filter(Boolean)
								.join(' · ')}
						</dd>
					{/if}

					{#if d.upstream_endpoint}
						<dt class="text-muted-foreground">上游接口</dt>
						<dd class="font-mono break-all">{d.upstream_endpoint}</dd>
					{/if}

					{#if d.status_code !== null || d.upstream_status !== null || d.upstream_priv_status !== null}
						<dt class="text-muted-foreground">状态码</dt>
						<dd class="tabular font-mono">
							{[
								d.status_code !== null ? `HTTP ${d.status_code}` : null,
								d.upstream_status !== null ? `status=${d.upstream_status}` : null,
								d.upstream_priv_status !== null ? `priv=${d.upstream_priv_status}` : null
							]
								.filter(Boolean)
								.join(' · ')}
						</dd>
					{/if}

					{#if d.upstream_quality !== null}
						<dt class="text-muted-foreground">上游音质</dt>
						<dd class="font-mono">{d.upstream_quality}</dd>
					{/if}

					{#if d.upstream_fail_process !== null}
						<dt class="text-muted-foreground">fail_process</dt>
						<dd class="font-mono break-all">{JSON.stringify(d.upstream_fail_process)}</dd>
					{/if}

					{#if d.human_verification}
						<dt class="text-muted-foreground">风控标记</dt>
						<dd class="font-medium text-danger">上游要求人机验证</dd>
					{/if}
				</dl>

				{#if reportedJson !== null}
					<div class="space-y-1 pt-1">
						<div class="flex items-center justify-between gap-2">
							<p class="text-muted-foreground">客户端上报档位 (reported_types)</p>
							<Button
								variant="ghost"
								size="sm"
								class="h-7 shrink-0 px-2 text-xs md:h-7"
								onclick={() => copyText('reported_types', reportedJson, '上报档位 JSON')}
							>
								{copiedField === 'reported_types' ? '已复制 JSON' : '复制 JSON'}
							</Button>
						</div>
						<pre class="max-h-48 overflow-auto rounded-lg border border-border bg-muted/60 p-2.5 font-mono text-xs leading-relaxed text-foreground">{reportedJson}</pre>
					</div>
				{/if}
			{:else if ev.event_type === 'token_refresh'}
				<dl class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
					<dt class="text-muted-foreground">刷新原因</dt>
					<dd>{labelOf(tokenRefreshReasonLabel, ev.detail.reason)}</dd>
					<dt class="text-muted-foreground">上游接口</dt>
					<dd class="font-mono break-all">{ev.detail.upstream_endpoint}</dd>
					{#if ev.detail.human_verification}
						<dt class="text-muted-foreground">风控标记</dt>
						<dd class="font-medium text-danger">上游要求人机验证</dd>
					{/if}
				</dl>
			{:else if ev.event_type === 'account_login'}
				<dl class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
					<dt class="text-muted-foreground">登录结果</dt>
					<dd>{labelOf(accountLoginOutcomeLabel, ev.detail.outcome)}</dd>
				</dl>
			{:else if ev.event_type === 'entitlement'}
				{@const d = ev.detail}
				{@const alreadyDone = isEntitlementAlreadyDone(d)}
				<dl class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
					<dt class="text-muted-foreground">动作</dt>
					<dd class="font-medium">{labelOf(entitlementActionLabel, d.action)}</dd>

					{#if d.reason}
						<dt class="text-muted-foreground">触发原因</dt>
						<dd>{labelOf(renewalReasonLabel, d.reason)}</dd>
					{/if}

					{#if d.upstream_day}
						<dt class="text-muted-foreground">上游日</dt>
						<dd class="tabular font-mono">{d.upstream_day} (UTC+8)</dd>
					{/if}

					{#if alreadyDone || d.outcome}
						<dt class="text-muted-foreground">结果</dt>
						<dd>
							<StatusBadge tone={badge.tone}>
								{alreadyDone ? '今日已完成' : labelOf(renewalOutcomeLabel, d.outcome)}
							</StatusBadge>
						</dd>
					{/if}

					{#if d.upstream_code !== undefined && d.upstream_code !== null}
						<dt class="text-muted-foreground">上游码</dt>
						<dd class="tabular font-mono">
							{d.upstream_code}{alreadyDone ? '（今日已达标幂等码）' : ''}
						</dd>
					{/if}

					{#if d.http_status !== undefined && d.http_status !== null}
						<dt class="text-muted-foreground">HTTP 状态</dt>
						<dd class="tabular font-mono">{d.http_status}</dd>
					{/if}

					{#if d.human_verification}
						<dt class="text-muted-foreground">风控标记</dt>
						<dd class="font-medium text-danger">上游要求人机验证</dd>
					{/if}

					{#if d.retry_at}
						<dt class="text-muted-foreground">重试窗口</dt>
						<dd class="tabular font-mono">{formatDateTime(d.retry_at)}</dd>
					{/if}

					{#if d.propagating_until}
						<dt class="text-muted-foreground">生效窗口</dt>
						<dd class="tabular font-mono">{formatDateTime(d.propagating_until)}</dd>
					{/if}

					{#if d.estimated_end_at}
						<dt class="text-muted-foreground">本地预计到期</dt>
						<dd class="tabular font-mono">{formatDateTime(d.estimated_end_at)}</dd>
					{/if}
				</dl>
			{/if}
		</section>

		<!-- 同 correlation_id 因果时间线 -->
		<section class="space-y-3 border-t border-border/70 pt-4">
			<div class="flex items-center justify-between gap-2">
				<h3 class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
					同关联链路 ({correlationLogs.length})
				</h3>
				<span class="font-mono text-caption text-muted-foreground">按发生顺序</span>
			</div>

			{#if loadingCorrelation}
				<div class="space-y-2">
					{#each [0, 1] as slot (slot)}
						<Skeleton class="h-12 w-full" />
					{/each}
				</div>
			{:else if correlationError}
				<Alert tone="danger">
					<p>{correlationError}</p>
					<Button variant="outline" size="sm" class="mt-2" onclick={onRetryCorrelation}>
						重试加载链路
					</Button>
				</Alert>
			{:else if correlationLogs.length === 0}
				<p class="text-muted-foreground">暂无关联链路记录。</p>
			{:else}
				<ol class="space-y-1.5">
					{#each correlationLogs as item (item.log_id)}
						{@const itemBadge = eventOutcomeBadge(item)}
						{@const itemHeadline = describeEventHeadline(item)}
						{@const isCurrent = item.log_id === ev.log_id}
						<li>
							<button
								type="button"
								class={cn(
									'flex w-full cursor-pointer items-start gap-2.5 rounded-lg border px-3 py-2 text-left transition-colors',
									isCurrent
										? 'border-primary/35 bg-primary-soft/50'
										: 'border-border/70 bg-background hover:bg-muted/40'
								)}
								onclick={() => onSelect(item.log_id)}
							>
								<span
									class={cn('mt-1.5 size-2 shrink-0 rounded-full', toneDotClass(itemBadge.tone))}
									aria-hidden="true"
								></span>
								<div class="min-w-0 flex-1">
									<div class="flex items-center justify-between gap-2">
										<span class="font-medium text-foreground">
											{labelOf(eventTypeLabel, item.event_type)} · {itemHeadline.title}
										</span>
										<span class="tabular shrink-0 font-mono text-caption text-muted-foreground">
											{formatClockTime(item.created_at)}
										</span>
									</div>
									{#if itemHeadline.subtitle}
										<p class="mt-0.5 truncate text-muted-foreground">{itemHeadline.subtitle}</p>
									{/if}
								</div>
							</button>
						</li>
					{/each}
				</ol>
			{/if}
		</section>
	</div>
{/snippet}

{#if event && isDesktop}
	<aside
		aria-label="事件详情检查器"
		class="hidden lg:sticky lg:top-20 lg:block lg:w-96 lg:shrink-0 lg:self-start xl:w-104"
	>
		<Card class="flex max-h-[calc(100vh-6rem)] flex-col overflow-hidden">
			<CardHeader class="shrink-0 border-b border-border/70 py-3.5">
				{@const badge = eventOutcomeBadge(event)}
				<div class="flex items-center justify-between gap-2">
					<div class="flex min-w-0 items-center gap-2">
						<StatusBadge tone="neutral">{labelOf(eventTypeLabel, event.event_type)}</StatusBadge>
						<StatusBadge tone={badge.tone} class="font-mono">{badge.label}</StatusBadge>
					</div>
					<Button variant="ghost" size="sm" onclick={onClose} aria-label="关闭事件详情">
						关闭
					</Button>
				</div>
			</CardHeader>
			<CardContent class="overflow-y-auto p-5">
				{@render inspectorBody(event)}
			</CardContent>
		</Card>
	</aside>
{/if}

{#if !isDesktop}
	<Dialog.Root
		open={Boolean(event)}
		onOpenChange={(open) => {
			if (!open) onClose();
		}}
	>
		{#if event}
			{@const badge = eventOutcomeBadge(event)}
			<Dialog.Content>
				<div class="flex shrink-0 items-center justify-between gap-2 border-b border-border/70 px-5 py-3.5">
					<div class="flex min-w-0 items-center gap-2">
						<Dialog.Title class="sr-only">
							{labelOf(eventTypeLabel, event.event_type)} 事件详情
						</Dialog.Title>
						<StatusBadge tone="neutral">{labelOf(eventTypeLabel, event.event_type)}</StatusBadge>
						<StatusBadge tone={badge.tone} class="font-mono">{badge.label}</StatusBadge>
					</div>
					<Dialog.Close>
						{#snippet child({ props })}
							<Button variant="ghost" size="sm" {...props}>关闭</Button>
						{/snippet}
					</Dialog.Close>
				</div>
				<div class="overflow-y-auto p-5">
					{@render inspectorBody(event)}
				</div>
			</Dialog.Content>
		{/if}
	</Dialog.Root>
{/if}

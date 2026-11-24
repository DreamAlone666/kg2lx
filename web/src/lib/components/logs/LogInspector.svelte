<script lang="ts">
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import { Alert, Button, Skeleton, StatusBadge } from '$lib/components/ui';
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
		formatDateTime,
		formatDateTimeMs,
		formatLatency,
		formatLogTimeMs,
		formatRelativeOffsetMs,
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
	 * 事件详情抽屉检查器：
	 *   - 桌面端（sm+）：右侧贴边全高抽屉，不受页头高度与外层滚动位置影响，不挤压主表列宽
	 *   - 移动端（< sm）：底部抽屉
	 *   - 统一经 bits-ui Dialog 管理滚动锁定、焦点陷阱与 Escape 关闭，支持页内上一条/下一条切换
	 */
	let {
		event,
		correlationLogs,
		loadingCorrelation,
		correlationError,
		prevLogId = null,
		nextLogId = null,
		onSelect,
		onRetryCorrelation,
		onClose
	}: {
		event: StoredEvent | null;
		correlationLogs: StoredEvent[];
		loadingCorrelation: boolean;
		correlationError: string | null;
		prevLogId?: string | null;
		nextLogId?: string | null;
		onSelect: (logId: string) => void;
		onRetryCorrelation: () => void;
		onClose: () => void;
	} = $props();

	onDestroy(() => {
		if (copyTimer) clearTimeout(copyTimer);
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
	{@const rawEventJson = JSON.stringify(ev, null, 2)}

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
				<dd class="tabular font-mono">{formatDateTimeMs(ev.created_at_ms)}</dd>

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
						<pre class="overflow-x-auto rounded-lg border border-border bg-muted/60 p-2.5 font-mono text-xs leading-relaxed text-foreground">{reportedJson}</pre>
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
						<dt class="text-muted-foreground">统计日期</dt>
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
							{d.upstream_code}{alreadyDone ? '（今日已领，无需重复）' : ''}
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
				{@const firstCreatedAt = correlationLogs[0]?.created_at_ms ?? ev.created_at_ms}
				<ol class="space-y-1.5">
					{#each correlationLogs as item, idx (item.log_id)}
						{@const itemBadge = eventOutcomeBadge(item)}
						{@const itemHeadline = describeEventHeadline(item)}
						{@const isCurrent = item.log_id === ev.log_id}
						{@const offsetText = idx === 0 ? '+0ms' : formatRelativeOffsetMs(item.created_at_ms - firstCreatedAt)}
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
										<span
											class="tabular shrink-0 font-mono text-caption text-muted-foreground"
											title={formatDateTimeMs(item.created_at_ms)}
										>
											{formatLogTimeMs(item.created_at_ms)}
										</span>
									</div>
									{#if itemHeadline.subtitle}
										<p class="mt-0.5 truncate text-muted-foreground">{itemHeadline.subtitle}</p>
									{/if}
									<div
										class="tabular mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 font-mono text-caption text-muted-foreground"
									>
										<span>{offsetText}</span>
										{#if item.latency_ms !== null}
											<span>耗时 {formatLatency(item.latency_ms)}</span>
										{/if}
										<span>#{item.seq}</span>
									</div>
								</div>
							</button>
						</li>
					{/each}
				</ol>
			{/if}
		</section>

		<!-- 原始 JSON -->
		<section class="space-y-2.5 border-t border-border/70 pt-4">
			<div class="flex flex-wrap items-center justify-between gap-2">
				<h3 class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">原始 JSON</h3>
				<div class="flex flex-wrap items-center gap-1.5">
					<Button
						variant="ghost"
						size="sm"
						class="h-7 shrink-0 px-2 text-xs md:h-7"
						onclick={() => copyText('raw_event', rawEventJson, '本条 JSON')}
					>
						{copiedField === 'raw_event' ? '已复制本条' : '复制本条 JSON'}
					</Button>
					{#if correlationLogs.length > 1}
						<Button
							variant="ghost"
							size="sm"
							class="h-7 shrink-0 px-2 text-xs md:h-7"
							onclick={() =>
								copyText('raw_chain', JSON.stringify(correlationLogs, null, 2), '整链 JSON')}
						>
							{copiedField === 'raw_chain' ? '已复制整链' : '复制整链 JSON'}
						</Button>
					{/if}
				</div>
			</div>

			<details>
				<summary
					class="cursor-pointer select-none rounded-md px-1 py-1 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground"
				>
					查看本条原始 JSON
				</summary>
				<pre class="mt-2 overflow-x-auto rounded-lg border border-border bg-muted/60 p-2.5 font-mono text-xs leading-relaxed text-foreground">{rawEventJson}</pre>
			</details>
		</section>
	</div>
{/snippet}

<Dialog.Root
	open={Boolean(event)}
	onOpenChange={(open) => {
		if (!open) onClose();
	}}
>
	{#if event}
		{@const badge = eventOutcomeBadge(event)}
		<Dialog.Content
			variant="drawer"
			aria-label="事件详情检查器"
			tabindex={-1}
			class="focus:outline-none"
			onOpenAutoFocus={(e) => {
				e.preventDefault();
				(e.currentTarget as HTMLElement | null)?.focus();
			}}
		>
			<div class="flex shrink-0 items-center justify-between gap-2 border-b border-border/70 px-5 py-3.5">
				<div class="flex min-w-0 items-center gap-2">
					<Dialog.Title class="sr-only">
						{labelOf(eventTypeLabel, event.event_type)} 事件详情
					</Dialog.Title>
					<StatusBadge tone="neutral">{labelOf(eventTypeLabel, event.event_type)}</StatusBadge>
					<StatusBadge tone={badge.tone} class="font-mono">{badge.label}</StatusBadge>
				</div>
				<div class="flex shrink-0 items-center gap-1">
					{#if prevLogId || nextLogId}
						<Button
							variant="ghost"
							size="sm"
							class="px-2"
							disabled={!prevLogId}
							onclick={() => {
								if (prevLogId) onSelect(prevLogId);
							}}
							aria-label="上一条事件"
						>
							上一条
						</Button>
						<Button
							variant="ghost"
							size="sm"
							class="px-2"
							disabled={!nextLogId}
							onclick={() => {
								if (nextLogId) onSelect(nextLogId);
							}}
							aria-label="下一条事件"
						>
							下一条
						</Button>
					{/if}
					<Dialog.Close>
						{#snippet child({ props })}
							<Button variant="ghost" size="sm" {...props}>关闭</Button>
						{/snippet}
					</Dialog.Close>
				</div>
			</div>
			<div class="min-h-0 flex-1 overflow-y-auto p-5">
				{@render inspectorBody(event)}
			</div>
		</Dialog.Content>
	{/if}
</Dialog.Root>

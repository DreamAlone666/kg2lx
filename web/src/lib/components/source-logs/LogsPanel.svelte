<script lang="ts">
	import { untrack } from 'svelte';
	import { Button, Card, CardContent, CardHeader, Skeleton, StatusBadge } from '$lib/components/ui';
	import { sourceLogsController } from '$lib/features/source-logs/controller.svelte';
	import { usePageVisible } from '$lib/hooks/use-page-visible.svelte';
	import {
		formatClockTime,
		formatLatency,
		labelOf,
		qualityTone,
		requestOutcomeTone,
		runtimeStageLabel,
		shortId
	} from '$lib/domain';
	import type { RuntimeLogListItem, RuntimeLogView } from '$lib/api/types';
	import { cn } from '$lib/utils.js';

	let { sourceId }: { sourceId: string } = $props();

	/** 用于让指示器诚实反映「轮询已暂停」，而不是继续显示「实时」。 */
	const pageVisible = usePageVisible();

	const views: { value: RuntimeLogView; label: string }[] = [
		{ value: 'all', label: '全部' },
		{ value: 'errors', label: '错误' }
	];

	const skeletonSlots = [0, 1, 2, 3];

	/** 新行高亮：关键帧定义在 layout.css，以内联 animation 引用以免被作用域样式裁剪。 */
	const ROW_FLASH = 'kg2lx-row-flash 900ms cubic-bezier(0.22, 1, 0.36, 1)';

	function rowFlashStyle(isNew: boolean) {
		return isNew ? `animation: ${ROW_FLASH}` : undefined;
	}

	// untrack：轮询启停会写入 controller 状态，不应成为本 effect 的依赖而自触发
	$effect(() => {
		if (!sourceId) return;
		untrack(() => sourceLogsController.startPolling(sourceId));
		return () => untrack(() => sourceLogsController.stopPolling());
	});
</script>

{#snippet songCell(log: RuntimeLogListItem)}
	{#if log.track_title}
		<span class="block truncate font-medium text-foreground">{log.track_title}</span>
		<span class="block truncate text-muted-foreground">
			{log.artist_name || '未知歌手'}{#if log.album_name}<span class="opacity-60">· {log.album_name}</span>{/if}
		</span>
	{:else}
		<span class="block truncate font-mono italic text-muted-foreground">无歌曲元数据</span>
		{#if log.album_audio_id}
			<span class="block truncate font-mono text-[0.7rem] text-muted-foreground">
				{log.album_audio_id}
			</span>
		{/if}
	{/if}
{/snippet}

{#snippet qualityCell(log: RuntimeLogListItem)}
	<StatusBadge tone={qualityTone(log.requested_quality)} class="font-mono">
		{log.requested_quality}
	</StatusBadge>
{/snippet}

{#snippet statusCell(log: RuntimeLogListItem)}
	<div class="flex flex-col items-start gap-1">
		<StatusBadge tone={requestOutcomeTone(log.ok)} class="font-mono">
			{log.ok ? 'OK' : log.error_code || 'ERR'}
		</StatusBadge>
		{#if !log.ok && log.error}
			<span class="max-w-32 truncate text-[0.7rem] text-danger" title={log.error}>{log.error}</span>
		{/if}
	</div>
{/snippet}

{#snippet diagCell(log: RuntimeLogListItem)}
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground">
		<span class="rounded bg-muted px-1.5 py-0.5 text-[0.7rem]">
			{labelOf(runtimeStageLabel, log.stage)}
		</span>
		<span class="tabular font-mono text-[0.7rem]">#{shortId(log.request_hash)}</span>
		{#if log.refresh_attempted}
			<span class="text-[0.7rem] font-medium text-warning">已刷新 Token</span>
		{/if}
		{#if log.retry_count > 0}
			<span class="text-[0.7rem] font-medium text-warning">重试 {log.retry_count}</span>
		{/if}
	</div>
{/snippet}

<Card class="mt-6 overflow-hidden">
	<CardHeader class="gap-0 border-b border-border/70">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex items-center gap-3">
				<h2 class="text-subheading">运行时日志</h2>

				{#if !pageVisible.visible}
					<StatusBadge tone="neutral" dot>已暂停</StatusBadge>
				{:else if sourceLogsController.loading}
					<StatusBadge tone="info" dot pulse>刷新中</StatusBadge>
				{:else}
					<StatusBadge tone="success" dot pulse>实时</StatusBadge>
				{/if}
			</div>

			<div class="flex items-center gap-2">
				<div class="flex rounded-lg bg-muted p-0.5" role="group" aria-label="日志筛选">
					{#each views as item (item.value)}
						{@const active = sourceLogsController.view === item.value}
						<button
							type="button"
							aria-pressed={active}
							class={cn(
								'cursor-pointer rounded-md px-3 py-1 text-xs font-medium transition-colors duration-150 ease-out-soft',
								active
									? 'bg-background text-foreground shadow-soft'
									: 'text-muted-foreground hover:text-foreground'
							)}
							onclick={() => sourceLogsController.setView(item.value)}
						>
							{item.label}
						</button>
					{/each}
				</div>

				<Button
					variant="ghost"
					size="sm"
					disabled={sourceLogsController.loading}
					onclick={() => sourceLogsController.refresh()}
				>
					刷新
				</Button>
			</div>
		</div>
	</CardHeader>

	<CardContent class="p-0">
		{#if sourceLogsController.error}
			<div class="p-8 text-center">
				<p class="text-sm leading-relaxed text-danger">
					加载日志失败：{sourceLogsController.error}
				</p>
				<Button size="sm" class="mt-4" onclick={() => sourceLogsController.refresh()}>重试</Button>
			</div>
		{:else if sourceLogsController.logs.length === 0 && sourceLogsController.loading}
			<div class="space-y-2 p-4">
				{#each skeletonSlots as slot (slot)}
					<Skeleton class="h-9 w-full" />
				{/each}
			</div>
		{:else if sourceLogsController.logs.length === 0}
			<div class="px-6 py-14 text-center">
				<p class="text-sm text-muted-foreground">
					{sourceLogsController.view === 'errors'
						? '暂无错误记录，一切正常。'
						: '暂无运行时流量记录，播放音乐后这里会出现取链日志。'}
				</p>
			</div>
		{:else}
			<!-- 桌面：表格承载多维对比，列宽固定以便纵向扫读 -->
			<div class="hidden md:block">
				<table class="w-full table-fixed text-left text-xs">
					<thead>
						<tr class="border-b border-border/70 text-muted-foreground">
							<th class="px-4 py-2.5 font-medium">歌曲信息</th>
							<th class="w-20 px-4 py-2.5 font-medium">音质</th>
							<th class="w-24 px-4 py-2.5 font-medium">状态</th>
							<th class="w-20 px-4 py-2.5 font-medium">时间</th>
							<th class="w-20 px-4 py-2.5 font-medium">延迟</th>
							<th class="w-56 px-4 py-2.5 font-medium">诊断</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-border/70">
						{#each sourceLogsController.logs as log (log.log_id)}
							{@const isNew = sourceLogsController.newLogIds.has(log.log_id)}
							<tr
								class="transition-colors hover:bg-muted/40"
								style={rowFlashStyle(isNew)}
							>
								<td class="min-w-0 px-4 py-2.5">
									<div class="flex flex-col gap-0.5">{@render songCell(log)}</div>
								</td>
								<td class="px-4 py-2.5">{@render qualityCell(log)}</td>
								<td class="px-4 py-2.5">{@render statusCell(log)}</td>
								<td class="tabular px-4 py-2.5 whitespace-nowrap text-muted-foreground">
									{formatClockTime(log.created_at)}
								</td>
								<td class="tabular px-4 py-2.5 whitespace-nowrap font-mono">
									{formatLatency(log.latency_ms)}
								</td>
								<td class="px-4 py-2.5">{@render diagCell(log)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>

			<!-- 移动端：改为分行卡片，避免最小宽度逼出横向滚动条 -->
			<ul class="divide-y divide-border/70 md:hidden">
				{#each sourceLogsController.logs as log (log.log_id)}
					{@const isNew = sourceLogsController.newLogIds.has(log.log_id)}
					<li class="space-y-2 px-4 py-3" style={rowFlashStyle(isNew)}>
						<div class="flex items-start justify-between gap-3">
							<div class="min-w-0 flex-1 text-xs">{@render songCell(log)}</div>
							{@render statusCell(log)}
						</div>

						<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
							{@render qualityCell(log)}
							<span class="tabular text-[0.7rem] text-muted-foreground">
								{formatClockTime(log.created_at)}
							</span>
							<span class="tabular font-mono text-[0.7rem] text-muted-foreground">
								{formatLatency(log.latency_ms)}
							</span>
							<span class="rounded bg-muted px-1.5 py-0.5 text-[0.7rem] text-muted-foreground">
								{labelOf(runtimeStageLabel, log.stage)}
							</span>
						</div>

						{#if log.error}
							<p class="text-[0.7rem] leading-relaxed text-danger">{log.error}</p>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</CardContent>
</Card>

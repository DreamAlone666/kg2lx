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
	import { EASE_OUT_SOFT } from '$lib/utils/motion';
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
	const ROW_FLASH = `kg2lx-row-flash 900ms ${EASE_OUT_SOFT}`;

	/**
	 * 状态列只放「成功 / 失败」两个短标签。
	 *
	 * 错误码曾经直接塞进这个胶囊，但 UPSTREAM_REQUEST_FAILED 这类长码需要约 138px，
	 * 远超 96px 的列宽；胶囊被压缩后文字会溢出到相邻的时间列，造成重叠。
	 * 教训是信息要按**列宽**分配而不是按重要性堆放：错误码与错误原因因此移到
	 * 更宽的「诊断」列，状态列只保留可一眼扫读的成败。
	 */

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
			<span class="block truncate font-mono text-caption text-muted-foreground">
				{log.album_audio_id}
			</span>
		{/if}
	{/if}
{/snippet}

{#snippet qualityCell(log: RuntimeLogListItem)}
	<!--
		max-w-full + truncate 是对未知音质串的防御：后端新增更长的取值时，
		它会在本列内显示省略号，而不是像错误码那样顶出边界压到邻列。
		title 保留完整取值，信息不丢失。
	-->
	<StatusBadge
		tone={qualityTone(log.requested_quality)}
		class="max-w-full truncate font-mono"
		title={log.requested_quality}
	>
		{log.requested_quality}
	</StatusBadge>
{/snippet}

{#snippet outcomeBadge(log: RuntimeLogListItem)}
	<StatusBadge tone={requestOutcomeTone(log.ok)} class="font-mono">
		{log.ok ? 'OK' : 'ERR'}
	</StatusBadge>
{/snippet}

{#snippet diagMeta(log: RuntimeLogListItem)}
	<!--
		只放「这次请求是谁」的标识：阶段、请求 hash，以及刷新与重试这两个过程标记。
		成败与错误已由状态列和 failureLine 承担，不在这一行重复。
	-->
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground">
		<span class="rounded bg-muted px-1.5 py-0.5 text-caption">
			{labelOf(runtimeStageLabel, log.stage)}
		</span>
		<span class="tabular font-mono text-caption">#{shortId(log.request_hash)}</span>
		{#if log.refresh_attempted}
			<span class="text-caption font-medium text-warning">已刷新 Token</span>
		{/if}
		{#if log.retry_count > 0}
			<span class="text-caption font-medium text-warning">重试 {log.retry_count}</span>
		{/if}
	</div>
{/snippet}

{#snippet failureLine(log: RuntimeLogListItem, wrap: boolean)}
	<!--
		失败的唯一主表达，只出现一次。
		同一次失败此前被表达三遍（状态列 ERR、诊断列错误码胶囊、诊断列错误原因），
		既冗余又把诊断列撑成 2~3 行、导致行高参差。

		flex-wrap + 错误原因的下限宽度是关键：`UPSTREAM_REQUEST_FAILED` 这类错误码在
		12px 等宽下要占掉约 166px，实测会把同行只剩 20px 的原因挤成「u…」——
		那是把信息截断成了噪音。给原因留出 min-w-24 的下限后，装不下时它会整段
		落到第二行并占满列宽，装得下（短错误码）时仍然同行，无需为两种情况写分支。

		wrap=true 用于移动端：宽度充裕，直接允许在词内折行，信息完整优先。
	-->
	<div class="flex min-w-0 flex-wrap items-start gap-x-1.5 gap-y-1">
		{#if log.error_code}
			<span class="shrink-0 rounded bg-danger-soft px-1.5 py-0.5 font-mono text-caption text-danger">
				{log.error_code}
			</span>
		{/if}
		{#if log.error}
			<span
				class={cn(
					'flex-1 text-caption text-danger',
					wrap ? 'min-w-0 break-words' : 'min-w-24 truncate'
				)}
				title={log.error}
			>
				{log.error}
			</span>
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
								'min-h-11 cursor-pointer rounded-md px-3.5 text-xs font-medium transition-colors duration-150 ease-out-soft md:min-h-7',
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
			<!--
				列宽按**内容类型**定档，主内容吃剩余空间。

				此前的列宽是按「刚好不溢出」反推出来的（350/112/96/80/80/256），
				结果是宽度排序与重要性排序相反：800px 视口实测歌曲信息只剩 126px，
				而诊断列独占 256px——最该一眼扫读的一列最窄。
				现在的规则是：固定列只取内容上限（内容长度已知或已被 truncate 防御），
				剩余全部交给歌曲信息，因为只有它是自由文本。

				诊断列刻意做成响应式（md 更窄、lg 才放宽）：诊断是次要信息，
				宽度吃紧时应当由它先让位，而不是挤压主内容。
			-->

			<!-- 桌面：表格承载多维对比，列宽固定以便纵向扫读 -->
			<div class="hidden md:block">
				<table class="w-full table-fixed text-left text-xs">
					<thead>
						<tr class="border-b border-border/70 text-muted-foreground">
							<th class="px-4 py-2.5 font-medium">歌曲信息</th>
							<th class="w-28 px-4 py-2.5 font-medium">音质</th>
							<th class="w-20 px-4 py-2.5 font-medium">状态</th>
							<th class="w-24 px-4 py-2.5 font-medium">时间</th>
							<th class="w-20 px-4 py-2.5 font-medium">延迟</th>
							<th class="w-40 px-4 py-2.5 font-medium lg:w-56">诊断</th>
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
								<td class="px-4 py-2.5">{@render outcomeBadge(log)}</td>
								<td class="tabular px-4 py-2.5 whitespace-nowrap text-muted-foreground">
									{formatClockTime(log.created_at)}
								</td>
								<td class="tabular px-4 py-2.5 whitespace-nowrap font-mono">
									{formatLatency(log.latency_ms)}
								</td>
								<td class="px-4 py-2.5">
									<div class="space-y-1">
										{@render diagMeta(log)}
										{#if !log.ok && (log.error_code || log.error)}
											{@render failureLine(log, false)}
										{/if}
									</div>
								</td>
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
							{@render outcomeBadge(log)}
						</div>

						<!-- 与桌面表格同一个失败主表达，只是允许折行：窄屏下用户更需要完整信息 -->
						{#if !log.ok && (log.error_code || log.error)}
							{@render failureLine(log, true)}
						{/if}

						<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
							{@render qualityCell(log)}
							<span class="tabular text-caption text-muted-foreground">
								{formatClockTime(log.created_at)}
							</span>
							<span class="tabular font-mono text-caption text-muted-foreground">
								{formatLatency(log.latency_ms)}
							</span>
						</div>

						<!-- 与桌面表格保持信息等价：阶段、请求 hash、刷新与重试标记
						     （错误码与原因由上面的 failureLine 承担，桌面端同样如此） -->
						{@render diagMeta(log)}
					</li>
				{/each}
			</ul>
		{/if}
	</CardContent>
</Card>

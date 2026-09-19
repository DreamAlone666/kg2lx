<script lang="ts">
	import { untrack } from 'svelte';
	import { Button, Card, CardContent, CardHeader, Skeleton, StatusBadge } from '$lib/components/ui';
	import { sourceLogsController } from '$lib/features/source-logs/controller.svelte';
	import { usePageVisible } from '$lib/hooks/use-page-visible.svelte';
	import {
		errorCodeLabel,
		formatClockTime,
		formatDateTime,
		formatLatency,
		formatLogTime,
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

/**
 * 「错误」视图下是否还需要状态列。
 *
 * 该视图已经保证每一行都是失败，红色的 ERR 胶囊在其中是零信息——它占着 80px
 * 却只是在重复筛选条件。整列让出去，宽度自动归给歌曲信息（table-fixed 里
 * 唯一的弹性列），比让诊断列去挤它更划算。
 */
const showOutcome = $derived(sourceLogsController.view !== 'errors');

/**
 * 表头单元格的基础样式。
 *
 * 分隔线用 `inset box-shadow` 而不是 `border-b`：`border-collapse: collapse` 下
 * 单元格的边框由**表格**绘制，不会跟着 sticky 的 thead 一起移动——实测滚到一半时
 * 表头与第一行数据之间完全没有线（用红色 3px border 探针复现：红线不出现在可见区），
 * 表头于是与数据糊成一片。换成 inset 阴影后红线精确落回吸顶位置。
 *
 * 用满强度的 `--color-border` 而非行分隔的 70%：表头是分区而不是行，略重的线
 * 反而符合分区语义。
 */
const TH = 'px-4 py-2.5 font-medium shadow-[inset_0_-1px_0_0_var(--color-border)]';

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

	/**
	 * 耗时列：0 不是「很快」，而是「没测到」。
	 *
	 * 服务端在「失败发生在发出请求之前」的两条路径（ensure_dfid / refresh_login 失败）
	 * 写的是占位 0，而真实的上游耗时也会向下取整成 0。渲染成 `0ms` 会被读作「极快」，
	 * 还会与成功行的 2ms 混为一谈——一个没有被测量过的值被显示成了测量结果。
	 * 统一显示 `—`：既不为未测量的值背书，也不丢掉 30ms / 86ms 这类真实测量。
	 */
	function latencyText(log: RuntimeLogListItem): string {
		return log.latency_ms > 0 ? formatLatency(log.latency_ms) : '—';
	}

	function latencyTitle(log: RuntimeLogListItem): string | undefined {
		return log.latency_ms > 0 ? undefined : '未记录到耗时';
	}

	/**
	 * 失败胶囊的悬停详情：原始错误码 + 服务端附带的错误信息。
	 *
	 * 胶囊上只放中文标签，原始码必须还能拿到——排查时要拿它去 grep 服务端日志，
	 * 而「中文标签」是给人读的中间层，不能成为唯一出口。
	 */
	function failureDetail(log: RuntimeLogListItem): string {
		const parts: string[] = [];
		if (log.error_code) parts.push(`错误码 ${log.error_code}`);
		if (log.error) parts.push(log.error);
		return parts.join('｜') || '请求失败';
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
		只放「这次请求是谁」的标识：阶段、请求指纹，以及刷新与重试这两个过程标记。
		成败与错误已由状态列和 failureBadge 承担，不在这一行重复。

		指纹的 title 是必要的：`#A0AEFE0` 这种 7 位截断值本身没有可操作性（不能粘贴、
		也没有第二个界面能关联），界面上从未说明它是干什么用的。而它其实承担着
		「两次请求内容是否完全相同」这个判断——实测 20 行里 #1A3CAAC 出现 4 次，
		语义是「同一请求发了 4 遍」，此前这份信息被渲染成了与其它行无异的噪音。
	-->
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground">
		<span class="rounded bg-muted px-1.5 py-0.5 text-caption">
			{labelOf(runtimeStageLabel, log.stage)}
		</span>
		<span
			class="tabular font-mono text-caption"
			title={`请求指纹 ${log.request_hash}｜相同指纹表示两次请求的内容完全相同`}
		>
			#{shortId(log.request_hash)}
		</span>
		{#if log.refresh_attempted}
			<span class="text-caption font-medium text-warning">已刷新 Token</span>
		{/if}
		{#if log.retry_count > 0}
			<span class="text-caption font-medium text-warning">重试 {log.retry_count}</span>
		{/if}
	</div>
{/snippet}

{#snippet failureBadge(log: RuntimeLogListItem)}
	<!--
		失败的唯一主表达，只出现一次。同一次失败此前被表达三遍（状态列 ERR、
		诊断列错误码胶囊、诊断列错误原因），既冗余又把诊断列撑成 2~3 行。

		现在收敛成一枚胶囊，两个字段合一：

		- `error` 不再单独占一行。它在服务端已被 sanitize 成常量
		  （runtime_log.rs 的 sanitize_runtime_log_error 把所有原因统一写成
		  "upstream error"），而它非 null 意味着 {#if log.error} 永远成立，
		  于是每个失败行都被一个常量吃掉整整一行。真实原因仍进 title。
		- 错误码换成中文标签（原始码进 title）。`UPSTREAM_*` 三个码在前 9 个字符上
		  完全相同，却各要 166px；换成「上游请求失败」后只要约 84px。

		顺带删掉一条死分支：此前是 flex-wrap + 原因 min-w-24(96px)，号称「装得下时
		同行」。实测等宽字符 6.71px、诊断列内容宽 192px，同行需要
		6.71n + 12 + 6 + 96 ≤ 192，即错误码不超过 11 字符；而 13 个真实错误码里最短的
		INTERNAL_ERROR 也有 14 字符。也就是说这条分支在真实取值域内一次都不会命中，
		调宽度是徒劳的——问题出在「用一列同时承载机器码与人读文案」这个前提上。
	-->
	<span
		class="rounded bg-danger-soft px-1.5 py-0.5 text-caption text-danger"
		title={failureDetail(log)}
	>
		{labelOf(errorCodeLabel, log.error_code, '请求失败')}
	</span>
{/snippet}

<Card class="mt-6 overflow-hidden">
	<CardHeader class="gap-0 border-b border-border/70">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
				<h2 class="text-subheading">运行时日志</h2>

				<!--
					顺序即优先级，不得随意调换：暂停压过一切，其次是失败，再次是进行中的拉取。
					error 分支是补上的——此前轮询是静默拉取（loading 始终为 false），
					于是一次瞬时失败之后徽章仍显示绿色「实时」，与紧邻的「日志刷新失败」
					同屏矛盾：徽章在替一个已经失败的状态背书。

					「实时」刻意不再脉冲。它是长期驻留的被动状态，用户无法据此做任何动作，
					无限循环动画只是持续的分心源；脉冲只留给用户正在等待其结束的状态
					（如「刷新中」与扫码页的「等待扫码」）。
				-->
				{#if !pageVisible.visible || sourceLogsController.paused}
					<StatusBadge tone="neutral" dot>已暂停</StatusBadge>
				{:else if sourceLogsController.error}
					<StatusBadge tone="danger" dot>刷新失败</StatusBadge>
				{:else if sourceLogsController.loading}
					<StatusBadge tone="info" dot pulse>刷新中</StatusBadge>
				{:else}
					<StatusBadge tone="success" dot>实时</StatusBadge>
				{/if}

				<!-- 暂停必须说清「你看到的是哪个时刻的数据」，否则暂停会退化成无声的数据过期 -->
				{#if sourceLogsController.paused && sourceLogsController.lastUpdatedAt}
					<span class="tabular text-xs whitespace-nowrap text-muted-foreground">
						数据截至 {formatClockTime(sourceLogsController.lastUpdatedAt)}
					</span>
				{/if}
			</div>

			<div class="flex items-center gap-2">
				<!--
					分段控件的外高必须与右侧 sm 档按钮一致：移动端 44px、md 以上 36px。
					做法是内层保持按钮级高度、外层只在 md 以上留 2px 内边距——
					移动端若同样留内边距，外框会涨到 48px 比按钮高 4px；
					反过来压缩内层又会把触控区压到 44px 以下，违背触屏工效下限。
				-->
				<div class="flex rounded-lg bg-muted px-0.5 md:p-0.5" role="group" aria-label="日志筛选">
					{#each views as item (item.value)}
						{@const active = sourceLogsController.view === item.value}
						<button
							type="button"
							aria-pressed={active}
							class={cn(
								'min-h-11 cursor-pointer rounded-md px-3.5 text-xs font-medium transition-colors duration-150 ease-out-soft md:min-h-8',
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

				<!--
					「暂停」是长驻页面的必要能力：日志每 5 秒整表替换，而「我想停下来看清一行」
					是高频需求，此前只能靠切走标签页（隐藏才会停）。
					手动「刷新」不受暂停影响——用户按了就该拉一次。
				-->
				<Button
					variant="ghost"
					size="sm"
					onclick={() => sourceLogsController.setPaused(!sourceLogsController.paused)}
				>
					{sourceLogsController.paused ? '继续' : '暂停'}
				</Button>

				<Button
					variant="ghost"
					size="sm"
					pending={sourceLogsController.loading}
					pendingLabel="刷新中…"
					onclick={() => sourceLogsController.refresh()}
				>
					刷新
				</Button>
			</div>
		</div>
	</CardHeader>

	<CardContent class="p-0">
		<!--
			失败降级：已有数据时只在表格上方加一条横幅，不接管整个面板。

			此前 `{#if error}` 的优先级高于表格，一次瞬时失败就会把 20 行日志连同
			滚动位置一起换成一个错误面板，而数据其实还在控制器里——用户正在看的
			内容被后台的一次抖动销毁了。这与音源列表页早已确立的原则一致
			（有数据时错误降级为横幅，见 sources/+page.svelte），此前只有日志没沿用。

			横幅刻意标注「以下为哪个时刻的数据」：降级之后用户仍需要知道
			自己看的是不是最新的一批，否则「保留数据」会变成「显示过期数据而不自知」。
		-->
		{#if sourceLogsController.error && sourceLogsController.logs.length > 0}
			<div
				class="flex flex-wrap items-start justify-between gap-3 border-b border-danger/20 bg-danger-soft/60 px-4 py-3"
			>
				<p class="text-xs leading-relaxed text-danger">
					日志刷新失败：{sourceLogsController.error}。{sourceLogsController.lastUpdatedAt
						? `以下为 ${formatClockTime(sourceLogsController.lastUpdatedAt)} 的数据。`
						: '以下为最近一次成功获取的数据。'}
				</p>
				<Button variant="ghost" size="sm" onclick={() => sourceLogsController.refresh()}>
					重试
				</Button>
			</div>
		{/if}

		{#if sourceLogsController.logs.length > 0}
			<!--
				列宽按**内容类型**定档，主内容吃剩余空间。

				此前的列宽是按「刚好不溢出」反推出来的（350/112/96/80/80/256），
				结果是宽度排序与重要性排序相反：800px 视口实测歌曲信息只剩 126px，
				而诊断列独占 256px——最该一眼扫读的一列最窄。
				现在的规则是：固定列只取内容上限（内容长度已知或已被 truncate 防御），
				剩余全部交给歌曲信息，因为只有它是自由文本。

				诊断列刻意做成响应式（md 更窄、lg 才放宽）：诊断是次要信息，
				宽度吃紧时应当由它先让位，而不是挤压主内容。

				lg 取 w-64(256px) 是被实测推出来的：诊断首行最宽的形态是
				「刷新登录 + #指纹 + 已刷新 Token」，DOM 实测需要 208px，而 w-56 只有
				224-32=192px 可用——差 16px 就会整行折成两行，把失败行从 65px 抬到 86px，
				与成功行 41px 拉出落差。

				md 则刻意停在 w-40(160px)：该档下 208px 的形态本来就装不下
				（w-48 的 192px 也不够），再加宽只会从歌曲信息列白拿走 32px。
				能装下的最宽形态是 100px 的「取链 + #指纹」，w-40 有 128px 富余。
			-->

			<!-- 桌面：表格承载多维对比，列宽固定以便纵向扫读 -->
			<div class="hidden md:block">
				<!--
					表头必须吸顶。卡片实测高 1909px（20 行 × 41~86px），而视口只有 900px：
					滚到第 15 行时「歌曲信息 / 音质 / 状态 / 时间 / 延迟 / 诊断」早已离屏，
					而列里大量是无自解释性的值（2ms、flac24bit、05-19 14:50:33），
					用户只能靠位置猜列义。给容器一个高度上限后表头才吸得住，
					顺带把「滚完 1909px 才能看到最新那行」变成「容器内滚动」。
					滚动容器放在 Card 的 overflow-hidden 之内，无需改动卡片的圆角裁剪。
				-->
				<div class="max-h-[70vh] overflow-y-auto">
					<table class="w-full table-fixed text-left text-xs">
						<thead class="sticky top-0 z-10 bg-card">
							<tr class="text-muted-foreground">
								<th class={TH}>歌曲信息</th>
								<th class="w-28 {TH}">音质</th>
								<!--
									「错误」视图下每一行都是失败，状态列的 ERR 是零信息，
									整列让给诊断（见 showOutcome）。
								-->
								{#if showOutcome}
									<th class="w-20 {TH}">状态</th>
								{/if}
								<!--
									时间列按最宽形态取档：跨天时是 `MM-DD HH:mm:ss` 共 14 字符，
									12px 下约 94px，加上左右 px-4 需要 126px，故取 w-36(144px)。
									此前按当天的 8 字符反推出 w-24(96px)，跨天行会溢出到延迟列。
								-->
								<th class="w-36 {TH}">时间</th>
								<th class="w-20 {TH}">延迟</th>
								<th class="w-40 {TH} lg:w-64">诊断</th>
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
									{#if showOutcome}
										<td class="px-4 py-2.5">{@render outcomeBadge(log)}</td>
									{/if}
									<td
										class="tabular px-4 py-2.5 whitespace-nowrap text-muted-foreground"
										title={formatDateTime(log.created_at)}
									>
										{formatLogTime(log.created_at)}
									</td>
									<td
										class="tabular px-4 py-2.5 whitespace-nowrap font-mono"
										title={latencyTitle(log)}
									>
										{latencyText(log)}
									</td>
									<td class="px-4 py-2.5">
										<div class="flex flex-col items-start gap-1">
											{@render diagMeta(log)}
											{#if !log.ok && (log.error_code || log.error)}
												{@render failureBadge(log)}
											{/if}
										</div>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</div>

			<!-- 移动端：改为分行卡片，避免最小宽度逼出横向滚动条 -->
			<ul class="divide-y divide-border/70 md:hidden">
				{#each sourceLogsController.logs as log (log.log_id)}
					{@const isNew = sourceLogsController.newLogIds.has(log.log_id)}
					<li class="space-y-2 px-4 py-3" style={rowFlashStyle(isNew)}>
						<div class="flex items-start justify-between gap-3">
							<div class="min-w-0 flex-1 text-xs">{@render songCell(log)}</div>
							{#if showOutcome}
								{@render outcomeBadge(log)}
							{/if}
						</div>

						<!-- 与桌面表格同一个失败主表达：中文标签在窄屏下同样不需要折行 -->
						{#if !log.ok && (log.error_code || log.error)}
							{@render failureBadge(log)}
						{/if}

						<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
							{@render qualityCell(log)}
							<span
								class="tabular text-caption text-muted-foreground"
								title={formatDateTime(log.created_at)}
							>
								{formatLogTime(log.created_at)}
							</span>
							<span
								class="tabular font-mono text-caption text-muted-foreground"
								title={latencyTitle(log)}
							>
								{latencyText(log)}
							</span>
						</div>

						<!-- 与桌面表格保持信息等价：阶段、请求 hash、刷新与重试标记
						     （错误码与原因由上面的 failureLine 承担，桌面端同样如此） -->
						{@render diagMeta(log)}
					</li>
				{/each}
			</ul>
		{:else if sourceLogsController.error}
			<!-- 零行 + 错误：此时没有可保留的内容，才让错误接管整个面板 -->
			<div class="p-8 text-center">
				<p class="text-sm leading-relaxed text-danger">
					加载日志失败：{sourceLogsController.error}
				</p>
				<Button size="sm" class="mt-4" onclick={() => sourceLogsController.refresh()}>重试</Button>
			</div>
		{:else if sourceLogsController.loading}
			<div class="space-y-2 p-4">
				{#each skeletonSlots as slot (slot)}
					<Skeleton class="h-9 w-full" />
				{/each}
			</div>
		{:else}
			<div class="px-6 py-14 text-center">
				<p class="text-sm text-muted-foreground">
					{sourceLogsController.view === 'errors'
						? '暂无错误记录，一切正常。'
						: '暂无运行时流量记录，播放音乐后这里会出现取链日志。'}
				</p>
			</div>
		{/if}
	</CardContent>
</Card>

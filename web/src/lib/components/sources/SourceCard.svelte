<script lang="ts">
	import { Badge, Card, CardContent, CardHeader, StatusBadge } from '$lib/components/ui';
	import { accountStatusLabel, formatDateTime, labelOf, providerLabel } from '$lib/domain';
	import { accountStatusTone, vipTone } from '$lib/domain/tone';
	import type { SourceSummary } from '$lib/api/types';

	/**
	 * 音源卡片。整卡可点，因此必须同时满足键盘可达（Card 的 interactive 变体
	 * 提供焦点环）与语义可达（外层 <a> 带 aria-label）。
	 *
	 * view-transition-name 与详情页的首个卡片保持一致，使「列表 → 详情」
	 * 呈现为卡片长大的共享元素过渡。
	 *
	 * 内容构成回答两个问题：**这个账号还活着吗**（账号状态胶囊）与
	 * **什么时候刷过**（最近刷新时间）。此前这两项都不在卡片上——前者要逐个点进
	 * 详情才能知道，后者展示的是 Source.updated_at（刷新不会改动它），
	 * 于是「刷新列表」之后卡片上的时间纹丝不动，列表失去了扫一眼做判断的价值。
	 *
	 * 刻意不再展示 source_id：它是内部主键，用户无法据此做任何操作，且被 truncate
	 * 后连完整读取都做不到。需要时地址栏里就是它。
	 */
	let { source }: { source: SourceSummary } = $props();

	/** 从未刷新过时不要拼成「最近刷新 从未」，那不是一句人话。 */
	const lastRefreshText = $derived(
		source.last_refresh_at ? `最近刷新 ${formatDateTime(source.last_refresh_at)}` : '从未刷新过'
	);

	/** 标题里已经含账号 ID 时不再重复展示；账号记录已消失时也无从补起。 */
	const showsAccountId = $derived(
		Boolean(source.userid) && !source.name.includes(source.userid)
	);
</script>

<!-- 是否要把账号 ID 单独报出来，与下面那行可见内容用的是同一条判断。 -->
<a
	href="/sources/{source.source_id}"
	class="block"
	style="view-transition-name: source-{source.source_id}"
	aria-label={showsAccountId
		? `${source.name} 账号 ${source.userid} 的详情`
		: `${source.name} 的详情`}
>
	<Card interactive class="h-full">
		<CardHeader class="gap-0 pb-3">
			<div class="flex flex-wrap items-center justify-between gap-x-2 gap-y-1.5">
				<Badge variant="secondary">{labelOf(providerLabel, source.provider)}</Badge>

				<!-- 源级标记放右侧：VIP 是账号级、已停用是源级，两者都属于「标记」而非「状态」，
				     真正的账号状态交给下面的胶囊单独一行承担。 -->
				<div class="flex shrink-0 items-center gap-1.5">
					{#if !source.enabled}
						<StatusBadge tone="neutral">已停用</StatusBadge>
					{/if}
					{#if source.vip_active}
						<StatusBadge tone={vipTone(true)} dot>VIP</StatusBadge>
					{/if}
				</div>
			</div>
			<h2 class="mt-3 truncate text-lg font-semibold tracking-tight" title={source.name}>
				{source.name}
			</h2>

			<!--
				只在标题里看不到账号 ID 时才补这一行。判据是「ID 是否已经可见」这条关于
				当前文本的事实，而不是「标题是否被改过」——后者只能拿 `name` 与
				`default_name` 比对得到，而 `default_name` 会随 `SOURCE_NAME_PREFIX` 漂移，
				换过前缀的部署上会误判（详见 `api/types.ts` 里该字段的说明）。
				userid 为空表示账号记录已不存在，此时没有可补的内容。
			-->
			{#if showsAccountId}
				<p class="truncate font-mono text-xs text-muted-foreground">账号 {source.userid}</p>
			{/if}
		</CardHeader>

		<CardContent class="space-y-2">
			<StatusBadge tone={accountStatusTone(source.account_status)} dot>
				{labelOf(accountStatusLabel, source.account_status)}
			</StatusBadge>

			<!-- tabular + nowrap：定长时间戳在网格里各卡片对齐，且不会被折行 -->
			<p class="tabular text-xs whitespace-nowrap text-muted-foreground">
				{lastRefreshText}
			</p>
		</CardContent>
	</Card>
</a>

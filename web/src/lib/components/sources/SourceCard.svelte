<script lang="ts">
	import { Badge, Card, CardContent, CardHeader, StatusBadge } from '$lib/components/ui';
	import { formatDateTime, labelOf, providerLabel } from '$lib/domain';
	import { vipTone } from '$lib/domain/tone';
	import type { SourceSummary } from '$lib/api/types';

	/**
	 * 音源卡片。整卡可点，因此必须同时满足键盘可达（Card 的 interactive 变体
	 * 提供焦点环）与语义可达（外层 <a> 带 aria-label）。
	 *
	 * view-transition-name 与详情页的首个卡片保持一致，使「列表 → 详情」
	 * 呈现为卡片长大的共享元素过渡。
	 */
	let { source }: { source: SourceSummary } = $props();
</script>

<a
	href="/sources/{source.source_id}"
	class="block"
	style="view-transition-name: source-{source.source_id}"
	aria-label="{labelOf(providerLabel, source.provider)} 账号 {source.userid} 的详情"
>
	<Card interactive class="h-full">
		<CardHeader class="gap-0 pb-3">
			<div class="flex items-center justify-between gap-2">
				<Badge variant="secondary">{labelOf(providerLabel, source.provider)}</Badge>
				{#if source.vip_active}
					<StatusBadge tone={vipTone(true)} dot>VIP</StatusBadge>
				{/if}
			</div>
			<h2 class="mt-3 truncate text-lg font-semibold tracking-tight">{source.userid}</h2>
		</CardHeader>

		<CardContent class="space-y-1">
			<p class="truncate font-mono text-xs text-muted-foreground" title={source.source_id}>
				ID: {source.source_id}
			</p>
			<p class="text-xs text-muted-foreground">更新于 {formatDateTime(source.updated_at)}</p>
		</CardContent>
	</Card>
</a>

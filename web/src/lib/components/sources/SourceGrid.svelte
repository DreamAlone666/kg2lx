<script lang="ts">
	import SourceCard from './SourceCard.svelte';
	import type { SourceSummary } from '$lib/api/types';

	/** 音源卡片网格：手机 1 列、平板 2 列、桌面 3 列。 */
	let { sources }: { sources: SourceSummary[] } = $props();
</script>

<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
	{#each sources as source (source.source_id)}
		<SourceCard {source} />
	{/each}
</div>

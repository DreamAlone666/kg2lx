<script lang="ts">
	import SourceCard from './SourceCard.svelte';
	import type { SourceSummary } from '$lib/api/types';

	/**
	 * 音源卡片网格。
	 *
	 * 断点列数（sm:2 / lg:3）有一个不明显的代价：**轨道数由断点决定，而不是由内容决定**。
	 * 只有 1 条音源时卡片仍然只占 1/3 宽，另外两条空轨道并排留着，
	 * 视觉上读起来像「还有内容没加载出来」。
	 *
	 * 改为 auto-fit：轨道数由可用宽度决定，空轨道被折叠，增删数据都不留空轨道。
	 *
	 * 但轨道**必须设上限**。只写 minmax(18rem, 1fr) 会让单条数据在外壳放宽到
	 * 1280px 后拉成一张 1232px 的通栏卡片——实测其视觉是「一行标签 + 右侧一个孤立的
	 * VIP」，比原来的 1/3 宽更糟。卡片是若干个短字段的聚合，宽度需要有上限才读得舒服；
	 * 宽出来的空间交给真正需要宽度的东西（运行日志表）。
	 */
	let { sources }: { sources: SourceSummary[] } = $props();
</script>

<!--
	justify-start 只在 sm 以上生效：手机端没有显式轨道，靠默认的 auto 轨道撑满整行；
	一旦在窄屏也 justify-start，轨道会退化成由内容决定的 max-content 宽度，
	卡片会缩到 242px 而不是撑满 358px。
-->
<div class="grid gap-4 sm:grid-cols-[repeat(auto-fit,minmax(18rem,24rem))] sm:justify-start">
	{#each sources as source (source.source_id)}
		<SourceCard {source} />
	{/each}
</div>

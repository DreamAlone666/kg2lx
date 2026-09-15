<script lang="ts">
	import { onMount } from 'svelte';
	import { Button, Card, Skeleton } from '$lib/components/ui';
	import { toneSurfaceClass } from '$lib/components/ui/tone-classes.js';
	import SourceGrid from '$lib/components/sources/SourceGrid.svelte';
	import { sourcesController } from '$lib/features/sources/controller.svelte';

	// 认证守卫已上提到 (app)/+layout.ts，页面只负责取数与状态编排
	onMount(() => sourcesController.loadSources());

	const skeletonSlots = [0, 1, 2];
</script>

<svelte:head>
	<title>音源管理 | kg2lx</title>
</svelte:head>

<div class="space-y-6">
	<header class="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
		<div>
			<h1 class="text-heading tracking-tight">音源管理</h1>
			<p class="mt-1 text-sm text-muted-foreground">
				管理已绑定的音乐账号，并获取导入洛雪音乐的脚本链接。
			</p>
		</div>

		<div class="flex shrink-0 items-center gap-2">
			<Button
				variant="outline"
				size="sm"
				disabled={sourcesController.loading}
				onclick={() => sourcesController.loadSources()}
			>
				刷新列表
			</Button>
			<Button href="/bind">添加音源</Button>
		</div>
	</header>

	{#if sourcesController.loading && sourcesController.sources.length === 0}
		<!-- 骨架而非转圈：减弱动态偏好下仍能静态表达内容将出现的位置 -->
		<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
			{#each skeletonSlots as slot (slot)}
				<Card class="p-6">
					<Skeleton class="h-5 w-20" />
					<Skeleton class="mt-4 h-6 w-32" />
					<Skeleton class="mt-4 h-3 w-full" />
					<Skeleton class="mt-2 h-3 w-24" />
				</Card>
			{/each}
		</div>
	{:else if sourcesController.sources.length === 0}
		{#if sourcesController.error}
			<div class="rounded-xl border p-5 {toneSurfaceClass('danger')}" role="alert">
				<p class="font-medium">加载音源失败</p>
				<p class="mt-1 text-sm leading-relaxed opacity-90">{sourcesController.error}</p>
				<Button
					variant="outline"
					size="sm"
					class="mt-4"
					onclick={() => sourcesController.loadSources()}
				>
					重试
				</Button>
			</div>
		{:else}
			<Card
				class="flex flex-col items-center justify-center gap-1 border-dashed px-6 py-16 text-center"
			>
				<p class="font-medium">还没有绑定任何音源</p>
				<p class="mt-1 max-w-sm text-sm leading-relaxed text-muted-foreground">
					绑定酷狗概念版账号后，这里会出现可直接导入洛雪音乐的脚本链接。
				</p>
				<Button href="/bind" class="mt-5">绑定第一个账号</Button>
			</Card>
		{/if}
	{:else}
		<!-- 已有数据时错误降级为横幅，不把用户已经看到的内容替换掉 -->
		{#if sourcesController.error}
			<div
				class="flex items-start justify-between gap-4 rounded-xl border p-4 {toneSurfaceClass(
					'danger'
				)}"
				role="alert"
			>
				<p class="text-sm leading-relaxed">{sourcesController.error}</p>
				<Button variant="ghost" size="sm" onclick={() => sourcesController.clearError()}>
					关闭
				</Button>
			</div>
		{/if}

		<SourceGrid sources={sourcesController.sources} />
	{/if}
</div>

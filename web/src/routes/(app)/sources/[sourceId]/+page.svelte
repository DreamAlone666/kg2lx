<script lang="ts">
	import { page } from '$app/state';
	import {
		Alert,
		Button,
		Card,
		CardContent,
		CardHeader,
		Skeleton,
		StatusBadge
	} from '$lib/components/ui';
	import CopyField from '$lib/components/sources/CopyField.svelte';
	import LogsPanel from '$lib/components/source-logs/LogsPanel.svelte';
	import { sourcesController } from '$lib/features/sources/controller.svelte';
	import { toast } from '$lib/state/toast.svelte';
	import {
		accountStatusLabel,
		accountStatusTone,
		formatDateTime,
		labelOf,
		providerLabel,
		vipTone
	} from '$lib/domain';

	let sourceId = $derived(page.params.sourceId ?? '');
	let source = $derived(sourcesController.currentSource);
	let refreshing = $derived(Boolean(sourcesController.refreshing[sourceId]));
	let pageTitle = $derived(source ? `${source.account.userid} | kg2lx` : '音源详情 | kg2lx');

	// 认证守卫已上提到 (app)/+layout.ts，页面只负责取数
	$effect(() => {
		const id = page.params.sourceId;
		if (!id) return;
		sourcesController.loadSource(id);
	});

	async function handleRefresh() {
		const ok = await sourcesController.refreshSource(sourceId);
		if (ok) {
			toast.success('账号状态已刷新。');
		} else {
			toast.error(sourcesController.error ?? '刷新失败，请稍后重试。');
		}
	}
</script>

<svelte:head>
	<title>{pageTitle}</title>
</svelte:head>

<div class="space-y-0">
	{#if !source && sourcesController.loading}
		<Card class="p-6">
			<Skeleton class="h-7 w-40" />
			<Skeleton class="mt-3 h-4 w-24" />
			<Skeleton class="mt-6 h-14 w-full" />
			<Skeleton class="mt-4 h-10 w-full" />
		</Card>
	{:else if !source}
		<Alert title="加载音源失败">
			<p class="leading-relaxed">
				{sourcesController.error ?? '未找到该音源，可能已被删除。'}
			</p>
			<Button variant="outline" size="sm" class="mt-4" href="/sources">返回音源列表</Button>
		</Alert>
	{:else}
		{#if sourcesController.error}
			<Alert class="mb-4">
				<div class="flex items-start justify-between gap-4">
					<p class="leading-relaxed">{sourcesController.error}</p>
					<Button variant="ghost" size="sm" onclick={() => sourcesController.clearError()}>
						关闭
					</Button>
				</div>
			</Alert>
		{/if}

		<Card style="view-transition-name: source-{sourceId}">
			<CardHeader class="border-b border-border/70">
				<div class="flex flex-wrap items-center justify-between gap-4">
					<div class="min-w-0">
						<h1 class="truncate text-2xl font-bold tracking-tight">
							{source.account.userid}
						</h1>
						<p class="mt-1 text-sm text-muted-foreground">
							平台：{labelOf(providerLabel, source.provider)}
						</p>
					</div>
					<Button variant="outline" onclick={handleRefresh} disabled={refreshing}>
						{refreshing ? '刷新中…' : '刷新 Token'}
					</Button>
				</div>
			</CardHeader>

			<CardContent class="space-y-8 p-6">
				<!--
					四段信息栅格与外壳同宽（main 的 max-w-5xl），因此这里必须凑满 4 列；
					只有 3 列时最后一格会留出大片空白，卡片显得松散。
				-->
				<div class="grid grid-cols-2 gap-6 sm:grid-cols-4">
					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">状态</p>
						<StatusBadge tone={accountStatusTone(source.account.status)} dot>
							{labelOf(accountStatusLabel, source.account.status)}
						</StatusBadge>
					</div>

					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							VIP 状态
						</p>
						<StatusBadge tone={vipTone(source.account.vip_active)} dot>
							{source.account.vip_active ? '已激活' : '未激活'}
						</StatusBadge>
					</div>

					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							最后刷新时间
						</p>
						<p class="tabular text-sm">{formatDateTime(source.account.last_refresh_at)}</p>
					</div>

					<div class="min-w-0 space-y-2">
						<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
							音源 ID
						</p>
						<p class="truncate font-mono text-xs text-muted-foreground" title={source.source_id}>
							{source.source_id}
						</p>
					</div>
				</div>

				<div class="space-y-3">
					<div>
						<h3 class="text-sm font-medium">脚本地址</h3>
						<p class="mt-1 text-xs leading-relaxed text-muted-foreground">
							将此 URL 导入洛雪音乐（LX Music）作为自定义音源。
						</p>
					</div>
					<CopyField value={source.script_url} />
				</div>

				{#if source.account.last_error}
					<div class="rounded-lg border border-danger/20 bg-danger-soft/60 p-4">
						<h3 class="text-xs font-bold tracking-wider text-danger uppercase">最后一次错误</h3>
						<p class="mt-1.5 text-sm leading-relaxed text-danger">
							{source.account.last_error}
						</p>
					</div>
				{/if}

				<div class="space-y-1">
					<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
						运行时 Token 预览
					</p>
					<p class="font-mono text-xs break-all text-muted-foreground/70">
						{source.runtime_token_preview}
					</p>
				</div>
			</CardContent>
		</Card>

		<LogsPanel {sourceId} />
	{/if}
</div>

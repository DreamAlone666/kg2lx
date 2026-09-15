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
						<div class="mt-1 flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
							<span>平台：{labelOf(providerLabel, source.provider)}</span>
							{#if !source.enabled}
								<StatusBadge tone="neutral">已停用</StatusBadge>
							{/if}
						</div>
					</div>
					<Button variant="outline" onclick={handleRefresh} disabled={refreshing}>
						{refreshing ? '刷新中…' : '刷新 Token'}
					</Button>
				</div>
			</CardHeader>

			<CardContent class="space-y-8 p-6">
				<!--
					三段信息栅格：轨道数必须与内容数一致。
					此前是 4 列，第 4 格「音源 ID」是为了填满 5xl 宽度而加的内部主键——
					用无效内容凑版式，且被 truncate 后连完整读取都做不到。移除后把轨道数
					一并降到 3，才不会留下死轨道；地址栏里本来就是同一个 ID。
				-->
				<div class="grid grid-cols-2 gap-6 sm:grid-cols-3">
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
						<!--
							窄屏用 text-xs 而不是 text-sm：19 字符的定长时间戳在 390px 的两列栅格里
							只有约 143px 可用，14px 下必然折行；12px 恰好单行放下，也让本格与同排的
							两个 StatusBadge（固定 text-xs）在字号上一致。
						-->
						<p class="tabular text-xs whitespace-nowrap sm:text-sm">
							{formatDateTime(source.account.last_refresh_at)}
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

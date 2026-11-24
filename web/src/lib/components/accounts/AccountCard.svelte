<script lang="ts">
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import { Badge, Button, Card, CardContent, CardHeader, StatusBadge } from '$lib/components/ui';
	import type { AccountItem } from '$lib/api/types';
	import {
		accountCardRenewalHint,
		accountStatusLabel,
		accountStatusTone,
		formatDateTime,
		labelOf,
		providerLabel,
		summarizeEntitlements,
		vipEntitlementEmphasis,
		VIP_ENTITLEMENT_TONE,
		vipProductLabel
	} from '$lib/domain';
	import { toast } from '$lib/state/toast.svelte';
	import { writeToClipboard } from '$lib/utils/clipboard';
	import { cn } from '$lib/utils.js';

	/**
	 * 账号卡片（stretched-link 模式）：
	 *   - 主链接覆盖整卡（after:inset-0），绝不在 <a> 内嵌 <button>
	 *   - 右下角「复制脚本」作为独立兄弟按钮置顶（z-10），不跳页即可一键复制主源脚本链接
	 */
	let { account }: { account: AccountItem } = $props();

	const primarySource = $derived(account.sources[0] ?? null);
	const title = $derived(
		primarySource?.name || `账号 ${account.userid}`
	);
	const showsUserid = $derived(Boolean(account.userid) && !title.includes(account.userid));
	const sourceCountLabel = $derived(
		account.sources.length > 1
			? `${account.sources.length} 个音源`
			: account.sources.length === 0
				? '暂无音源'
				: null
	);
	const subtitleParts = $derived(
		[showsUserid ? `账号 ${account.userid}` : null, sourceCountLabel].filter(Boolean)
	);

	const snapshot = $derived(summarizeEntitlements(account));
	const renewalHint = $derived(accountCardRenewalHint(account.renewal));
	const lastRefreshText = $derived(
		account.last_refresh_at ? `最近刷新 ${formatDateTime(account.last_refresh_at)}` : '从未刷新过'
	);

	type CopyState = 'idle' | 'copied' | 'failed';
	let copyState = $state<CopyState>('idle');
	let copyResetTimer: ReturnType<typeof setTimeout> | null = null;

	async function handleCopyScript() {
		if (!primarySource) return;
		const ok = await writeToClipboard(primarySource.script_url);
		copyState = ok ? 'copied' : 'failed';

		if (ok) {
			toast.success('脚本链接已复制，可在洛雪音乐中作为自定义音源导入。');
		} else {
			toast.error('复制失败，请进入账号详情手动复制。');
		}

		if (copyResetTimer) clearTimeout(copyResetTimer);
		copyResetTimer = setTimeout(() => {
			copyState = 'idle';
		}, 2000);
	}

	onDestroy(() => {
		if (copyResetTimer) clearTimeout(copyResetTimer);
	});

	const hintColorClass: Record<'muted' | 'info' | 'warning' | 'danger' | 'success' | 'neutral', string> = {
		muted: 'text-muted-foreground',
		neutral: 'text-muted-foreground',
		info: 'text-info',
		warning: 'text-warning',
		danger: 'font-medium text-danger',
		success: 'text-success',
	};
</script>

<Card
	interactive
	class="relative flex h-full flex-col"
	style="view-transition-name: account-{account.account_id}"
>
	<CardHeader class="gap-0 pb-3">
		<div class="flex flex-wrap items-center justify-between gap-x-2 gap-y-1.5">
			<Badge variant="secondary">{labelOf(providerLabel, account.provider)}</Badge>

			<div class="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
				{#if account.sources.some((s) => s.quality_fallback)}
					<Badge
						variant="secondary"
						title="音质自动降级已开启：上游拒绝所选音质时会自动改请求更低档位，下载同样生效"
					>
						自动降级
					</Badge>
				{/if}
				{#each snapshot.state === 'failed' ? [] : snapshot.named as entitlement, index (`${entitlement.product_type}:${index}`)}
					<StatusBadge
						tone={VIP_ENTITLEMENT_TONE}
						emphasis={vipEntitlementEmphasis(entitlement.product_type)}
					>
						{labelOf(vipProductLabel, entitlement.product_type)}
					</StatusBadge>
				{/each}
			</div>
		</div>

		<h2 class="mt-3 text-lg font-semibold tracking-tight">
			<a
				href={resolve(`/accounts/${account.account_id}`)}
				class="block truncate rounded-sm after:absolute after:inset-0 after:z-0 after:rounded-xl after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-offset-2 focus-visible:after:outline-ring"
				title={title}
				aria-label={showsUserid ? `${title} 账号 ${account.userid} 的详情` : `${title} 的详情`}
			>
				{title}
			</a>
		</h2>

		{#if subtitleParts.length > 0}
			<p class="truncate font-mono text-xs text-muted-foreground">
				{subtitleParts.join(' · ')}
			</p>
		{/if}
	</CardHeader>

	<CardContent class="flex flex-1 flex-col justify-between gap-3">
		<div class="space-y-2">
			<StatusBadge tone={accountStatusTone(account.status)} dot>
				{labelOf(accountStatusLabel, account.status)}
			</StatusBadge>

			{#if snapshot.state === 'failed'}
				<p class="text-xs text-danger">权益同步失败 · 在详情重试</p>
			{:else if snapshot.state === 'unknown'}
				<p class="text-xs text-muted-foreground">权益待同步 · 在详情同步</p>
			{:else if snapshot.state === 'empty'}
				<p class="text-xs text-muted-foreground">上次确认无在期权益</p>
			{:else if snapshot.hasUnnamedActive && snapshot.named.length === 0}
				<p class="text-xs text-muted-foreground">上次记录有在期权益</p>
			{/if}

			<p class={cn('text-xs leading-snug', hintColorClass[renewalHint.tone])}>
				{renewalHint.text}
			</p>
		</div>

		<div class="flex items-center justify-between gap-2 border-t border-border/60 pt-3">
			<span class="tabular min-w-0 truncate text-xs text-muted-foreground">
				{lastRefreshText}
			</span>

			{#if primarySource}
				<Button
					variant={copyState === 'failed' ? 'destructive' : 'outline'}
					size="sm"
					class="relative z-10 shrink-0"
					onclick={handleCopyScript}
					aria-label={`复制 ${primarySource.name} 的脚本链接`}
				>
					{#if copyState === 'copied'}
						<svg
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							class="size-3.5"
							aria-hidden="true"
						>
							<path d="M20 6 9 17l-5-5" />
						</svg>
						已复制
					{:else if copyState === 'failed'}
						复制失败
					{:else}
						复制脚本
					{/if}
				</Button>
			{/if}
		</div>
	</CardContent>
</Card>

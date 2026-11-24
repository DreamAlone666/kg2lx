<script lang="ts">
	import { untrack } from 'svelte';
	import { Button, Card, CardContent, CardHeader, StatusBadge } from '$lib/components/ui';
	import { Switch } from '$lib/components/ui/switch';
	import { usePageVisible } from '$lib/hooks/use-page-visible.svelte';
	import type { AccountItem } from '$lib/api/types';
	import type { AccountAction } from '$lib/features/accounts/controller.svelte';
	import {
		activeRenewalDeadline,
		describeRenewalState,
		entitlementExpiringTone,
		formatClockTime,
		formatDateTime,
		formatDuration,
		labelOf,
		renewalActionControl,
		renewalDeadlineTarget,
		renewalStateLabel,
		renewalStateTone,
		renewalStepLabel,
		renewalStepTone,
		summarizeEntitlements,
		vipEntitlementEmphasis,
		VIP_ENTITLEMENT_TONE,
		vipProductLabel
	} from '$lib/domain';
	import { cn } from '$lib/utils.js';

	/**
	 * 会员权益工作台卡片：
	 *   - 上半区：100% 只读的上游确认快照（account.entitlements）
	 *   - 下半区：bg-muted/50 内嵌面板，隔离本地自动领取 VIP 调度、2 步微型进度、
	 *            9 态状态与可见性感知倒计时、本地预计到期（不覆盖上方快照）
	 */
	let {
		account,
		pendingAction,
		savingAutoClaim,
		onSync,
		onClaim,
		onToggleAutoClaim,
		onDeadlineElapsed
	}: {
		account: AccountItem;
		pendingAction: AccountAction | undefined;
		savingAutoClaim: boolean;
		onSync: () => void;
		onClaim: () => void;
		onToggleAutoClaim: (next: boolean) => void;
		onDeadlineElapsed: () => void;
	} = $props();

	const pageVisible = usePageVisible();
	const snapshot = $derived(summarizeEntitlements(account));
	const renewal = $derived(account.renewal);
	const svipEndAt = $derived(
		account.entitlements?.find((item) => item.product_type === 'svip')?.end_at ?? null
	);
	const showEstimatedEndAt = $derived(
		renewal.estimated_end_at !== null &&
			renewal.estimated_end_at !== svipEndAt &&
			(svipEndAt === null ||
				renewal.svip_confirmation_required ||
				svipEndAt < renewal.estimated_end_at - 60)
	);

	let nowSec = $state(Math.floor(Date.now() / 1000));
	let triggeredDeadlineKey: string | null = null;

	const deadline = $derived(activeRenewalDeadline(renewal, nowSec));
	const control = $derived(renewalActionControl(renewal, nowSec));
	const stateDescription = $derived(describeRenewalState(renewal, nowSec));

	const rawTargetUntil = $derived(renewalDeadlineTarget(renewal)?.until ?? null);

	// 受控开关本地展示值：仅在非保存态时与服务端 renewal.enabled 对齐
	let autoClaimChecked = $state(untrack(() => account.renewal.enabled));
	$effect(() => {
		if (savingAutoClaim) return;
		autoClaimChecked = renewal.enabled;
	});

	$effect(() => {
		const until = rawTargetUntil;
		const accountId = account.account_id;
		if (until === null || !Number.isFinite(until) || !pageVisible.visible) return;

		let timer: ReturnType<typeof setInterval> | undefined;
		const key = `${accountId}:${until}`;
		const tick = () => {
			const now = Date.now();
			nowSec = Math.floor(now / 1000);
			// Give the server clock a short margin before requesting the next state.
			if (now < until * 1000 + 1500) return;
			if (timer !== undefined) clearInterval(timer);
			if (triggeredDeadlineKey !== key) {
				triggeredDeadlineKey = key;
				untrack(() => onDeadlineElapsed());
			}
		};
		timer = setInterval(tick, 1000);
		tick();
		return () => clearInterval(timer);
	});

	const uid = $props.id();
	const autoTitleId = `auto-renewal-title-${uid}`;
	const autoDescId = `auto-renewal-desc-${uid}`;
</script>

<Card>
	<CardHeader class="border-b border-border/70 pb-4">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-subheading">会员权益</h2>
			<Button
				variant="outline"
				size="sm"
				pending={pendingAction === 'sync'}
				pendingLabel="同步中…"
				disabled={Boolean(pendingAction)}
				onclick={onSync}
			>
				同步权益
			</Button>
		</div>
	</CardHeader>

	<CardContent class="space-y-5 p-6">
		<!-- 上半区：上游确认快照（100% 只读 entitlements） -->
		<section aria-label="上游确认会员快照" class="space-y-3">
			{#if snapshot.state === 'failed'}
				<p class="text-sm text-danger">同步失败。请重试同步权益。</p>
			{:else if snapshot.state === 'unknown'}
				<p class="text-sm text-muted-foreground">待同步。点击「同步权益」读取上游记录。</p>
			{/if}

			{#if account.entitlements !== undefined}
				<p class="tabular text-xs text-muted-foreground">
					{account.entitlements_synced_at
						? `上次同步 ${formatDateTime(account.entitlements_synced_at)}`
						: '历史快照 · 同步时间未知'}
				</p>

				{#if snapshot.named.length > 0}
					<ul class="flex flex-wrap items-center gap-x-6 gap-y-2">
						{#each snapshot.named as entitlement, index (`${entitlement.product_type}:${index}`)}
							{@const expiringTone = entitlementExpiringTone(entitlement.end_at)}
							<li class="flex flex-wrap items-center gap-x-2.5 gap-y-1">
								<StatusBadge
									tone={VIP_ENTITLEMENT_TONE}
									emphasis={vipEntitlementEmphasis(entitlement.product_type)}
								>
									{labelOf(vipProductLabel, entitlement.product_type)}
								</StatusBadge>
								<span class="tabular text-xs whitespace-nowrap text-muted-foreground">
									到期 {formatDateTime(entitlement.end_at, '未知')}
								</span>
								{#if expiringTone}
									<StatusBadge tone={expiringTone}>即将到期</StatusBadge>
								{/if}
							</li>
						{/each}
					</ul>
				{/if}

				{#if snapshot.hasUnnamedActive && snapshot.named.length === 0}
					<p class="text-xs text-muted-foreground">上次记录有在期权益，但均不在已命名档位内。</p>
				{:else if snapshot.hasUnnamedActive}
					<p class="text-xs text-muted-foreground">上次记录还有未命名的在期权益。</p>
				{:else if snapshot.named.length === 0}
					<p class="text-sm text-muted-foreground">上次同步已确认无在期权益。</p>
				{/if}
			{/if}
		</section>

		<!-- 下半区：明度内嵌面板，清晰隔离本地自动领取 VIP 与上游快照 -->
		<section
			aria-labelledby={autoTitleId}
			class="space-y-4 rounded-xl bg-muted/50 p-4"
		>
			<div class="flex flex-wrap items-start justify-between gap-4">
				<div class="min-w-0 flex-1 space-y-1">
					<h3 id={autoTitleId} class="text-sm font-medium">自动领取 VIP</h3>
					<p id={autoDescId} class="text-xs leading-relaxed text-muted-foreground">
						播放歌曲时，若概念版VIP 剩余不足 48 小时或因过期被拒，将在后台自动领取并升级（无定时任务）；过期补领后约需 2 分钟生效。
					</p>
				</div>

				<Switch
					bind:checked={autoClaimChecked}
					onCheckedChange={onToggleAutoClaim}
					disabled={savingAutoClaim}
					aria-labelledby={autoTitleId}
					aria-describedby={autoDescId}
					class="touch-target text-xs font-medium"
				>
					<span class="text-foreground">
						{savingAutoClaim ? '保存中…' : renewal.enabled ? '已开启' : '已关闭'}
					</span>
				</Switch>
			</div>

			<!-- 9 态状态徽章 + 统计日期 -->
			<div class="flex flex-wrap items-center justify-between gap-2 border-t border-border/60 pt-3">
				<div class="flex flex-wrap items-center gap-2">
					<StatusBadge
						tone={renewalStateTone(renewal.state)}
						dot
						pulse={renewal.state === 'propagating'}
					>
						{labelOf(renewalStateLabel, renewal.state)}
					</StatusBadge>
					<span class="tabular text-xs text-muted-foreground">
						统计日期 {renewal.upstream_day} (UTC+8)
					</span>
				</div>
			</div>

			<!-- 两步微型进度指示 -->
			<div class="flex flex-wrap items-center gap-2 text-xs">
				<div class="inline-flex items-center gap-1.5 rounded-lg bg-background/80 px-2.5 py-1.5">
					<span class="text-muted-foreground">① 领取畅听VIP</span>
					<StatusBadge tone={renewalStepTone(renewal.claim)}>
						{labelOf(renewalStepLabel, renewal.claim)}
					</StatusBadge>
				</div>
				<span class="text-muted-foreground" aria-hidden="true">→</span>
				<div class="inline-flex items-center gap-1.5 rounded-lg bg-background/80 px-2.5 py-1.5">
					<span class="text-muted-foreground">② 升级概念版VIP</span>
					<StatusBadge tone={renewalStepTone(renewal.upgrade)}>
						{labelOf(renewalStepLabel, renewal.upgrade)}
					</StatusBadge>
				</div>
			</div>

			<!-- 状态说明 + 实时倒计时 + 本地预计到期 -->
			<div class="space-y-1.5">
				<p class="text-xs leading-relaxed text-foreground/90">
					{stateDescription}
				</p>

				{#if deadline}
					<p class="tabular text-xs font-medium text-info">
						{deadline.label}：{formatDuration(deadline.remainingSeconds)}（预计至 {deadline.remainingSeconds >= 3600 ? formatDateTime(deadline.until) : formatClockTime(deadline.until)}）
					</p>
				{/if}

				{#if showEstimatedEndAt}
					<p class="tabular text-xs text-muted-foreground">
						本地预计到期 {formatDateTime(renewal.estimated_end_at)}（推算值，以上方同步快照为准）
					</p>
				{/if}
			</div>

			<!-- 手动触发按钮 -->
			<div class="flex flex-wrap items-center justify-between gap-3 pt-1">
				<Button
					variant="outline"
					size="sm"
					pending={pendingAction === 'claim'}
					pendingLabel="执行中…"
					disabled={Boolean(pendingAction) || control.disabled}
					onclick={onClaim}
				>
					{control.label}
				</Button>

				{#if renewal.svip_confirmation_required && (renewal.state === 'awaiting_upgrade' || renewal.state === 'propagating')}
					<span class="text-xs text-muted-foreground">完成后可通过上方「同步权益」确认到账</span>
				{/if}
			</div>
		</section>
	</CardContent>
</Card>

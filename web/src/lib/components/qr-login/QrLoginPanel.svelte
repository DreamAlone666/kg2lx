<script lang="ts">
	import {
		Alert,
		Button,
		Card,
		CardContent,
		CardHeader,
		Skeleton,
		StatusBadge
	} from '$lib/components/ui';
	import LoginStepTimeline from './LoginStepTimeline.svelte';
	import { qrLoginController } from '$lib/features/qr-login/controller.svelte';
	import { buildLoginSteps } from '$lib/features/qr-login/steps';
	import { formatCountdown, labelOf, sessionStatusLabel } from '$lib/domain';
	import { sessionStatusTone } from '$lib/domain/tone';
	import { cn } from '$lib/utils.js';

	/**
	 * 扫码绑定面板。
	 *
	 * 这段交互的关键约束是**跨设备**：用户手持手机扫码，眼睛看的是桌面屏幕。
	 * 因此二维码足够大、状态推进可视、剩余有效期可数——否则用户只能反复试错。
	 *
	 * 重启动作由页面注入（onRestart），因为「重新获取」还要同步 URL 中的 session_id，
	 * 那属于路由职责，不属于展示组件。
	 */
	let { onRestart }: { onRestart: () => void | Promise<void> } = $props();

	/** 剩余 30 秒起转为琥珀色告警，给用户留出反应时间。 */
	const EXPIRING_SOON_SECONDS = 30;

	const RING_RADIUS = 7;
	const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

	const steps = $derived(buildLoginSteps(qrLoginController.status, qrLoginController.stalledAt));
	const remaining = $derived(qrLoginController.remainingSeconds);
	const total = $derived(qrLoginController.totalSeconds);
	const expiringSoon = $derived(remaining !== null && remaining <= EXPIRING_SOON_SECONDS);

	const ringOffset = $derived(
		remaining !== null && total !== null
			? RING_CIRCUMFERENCE * (1 - Math.min(1, remaining / total))
			: RING_CIRCUMFERENCE
	);
</script>

<Card>
	<CardHeader class="text-center">
		<h1 class="text-heading tracking-tight">绑定酷狗账号</h1>
		<p class="mt-1 text-sm text-muted-foreground">请使用酷狗概念版 App 扫描下方二维码。</p>
	</CardHeader>

	<CardContent class="space-y-6">
		{#if !qrLoginController.sessionId}
			<div class="space-y-5">
				<div
					class="mx-auto grid size-44 place-items-center rounded-2xl border border-dashed border-border bg-muted/40"
				>
					<svg
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="1.5"
						stroke-linecap="round"
						stroke-linejoin="round"
						class="size-11 text-muted-foreground"
						aria-hidden="true"
					>
						<rect x="3" y="3" width="7" height="7" rx="1" />
						<rect x="14" y="3" width="7" height="7" rx="1" />
						<rect x="3" y="14" width="7" height="7" rx="1" />
						<path d="M14 14h3v3h-3z" />
						<path d="M20 14v2" />
						<path d="M17 20h4" />
						<path d="M14 20v1" />
					</svg>
				</div>

				{#if qrLoginController.message}
					<Alert>{qrLoginController.message}</Alert>
				{/if}

				<Button
					size="lg"
					class="w-full"
					disabled={qrLoginController.starting}
					onclick={onRestart}
				>
					{qrLoginController.starting ? '获取中…' : '获取二维码'}
				</Button>
			</div>
		{:else}
			<div class="mx-auto w-fit">
				<div
					class={cn(
						'rounded-2xl border border-border bg-background p-4 shadow-soft transition-opacity duration-200 ease-out-soft',
						qrLoginController.isTerminal && 'opacity-40 grayscale'
					)}
				>
					{#if qrLoginController.qrBase64}
						<img src={qrLoginController.qrBase64} alt="酷狗登录二维码" class="size-56" />
					{:else if qrLoginController.qrUrl}
						<div
							class="grid size-56 place-items-center rounded-lg bg-muted p-4 text-center font-mono text-xs break-all text-muted-foreground"
						>
							{qrLoginController.qrUrl}
						</div>
					{:else}
						<Skeleton class="size-56" />
					{/if}
				</div>
			</div>

			<div class="flex items-center justify-center gap-3">
				<StatusBadge
					tone={sessionStatusTone(qrLoginController.status)}
					dot
					pulse={!qrLoginController.isTerminal}
				>
					{labelOf(sessionStatusLabel, qrLoginController.status)}
				</StatusBadge>

				{#if remaining !== null && !qrLoginController.isTerminal}
					<span
						class={cn(
							'tabular inline-flex items-center gap-1.5 text-xs',
							expiringSoon ? 'text-warning' : 'text-muted-foreground'
						)}
					>
						<svg viewBox="0 0 16 16" class="size-4 -rotate-90" aria-hidden="true">
							<circle
								cx="8"
								cy="8"
								r={RING_RADIUS}
								fill="none"
								stroke="currentColor"
								stroke-width="2"
								class="opacity-20"
							/>
							<circle
								cx="8"
								cy="8"
								r={RING_RADIUS}
								fill="none"
								stroke="currentColor"
								stroke-width="2"
								stroke-linecap="round"
								stroke-dasharray={RING_CIRCUMFERENCE}
								stroke-dashoffset={ringOffset}
								class="transition-[stroke-dashoffset] duration-1000 ease-linear"
							/>
						</svg>
						剩余 {formatCountdown(remaining)}
					</span>
				{/if}
			</div>

			<LoginStepTimeline {steps} />

			{#if qrLoginController.status === 'bound' && qrLoginController.boundSource}
				<Alert tone="success" title="绑定成功" role="status">
					<p class="leading-relaxed">
						已绑定账号 {qrLoginController.boundSource.account.userid}，正在跳转到音源详情…
					</p>
				</Alert>
			{:else if qrLoginController.message}
				<Alert>{qrLoginController.message}</Alert>
			{/if}

			<div class="text-center">
				<Button
					variant="ghost"
					size="sm"
					disabled={qrLoginController.starting}
					onclick={onRestart}
				>
					重置并重试
				</Button>
			</div>
		{/if}
	</CardContent>
</Card>

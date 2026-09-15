<script lang="ts">
	import type { LoginStep, StepState } from '$lib/features/qr-login/steps';
	import { cn } from '$lib/utils.js';

	/**
	 * 竖向步骤时间轴：纯展示组件，只消费 steps.ts 推导出的有序步骤。
	 *
	 * 动效全部有语义，不做装饰性动画：
	 *   - 当前步的圆点呼吸 → 表示「正在等待用户的某个动作」
	 *   - 已完成步的连线从 0 生长到满 → 表示进度确实向前推进了
	 *   - 终止态整条转为失败色 → 表示流程在此中断
	 */
	let { steps }: { steps: LoginStep[] } = $props();

	const markerClass: Record<StepState, string> = {
		done: 'border-primary bg-primary text-primary-foreground',
		active: 'border-primary bg-primary-soft text-primary-soft-foreground',
		failed: 'border-danger bg-danger text-white',
		pending: 'border-border bg-background text-muted-foreground'
	};

	const labelClass: Record<StepState, string> = {
		done: 'text-foreground',
		active: 'font-medium text-foreground',
		failed: 'font-medium text-danger',
		pending: 'text-muted-foreground'
	};
</script>

<ol class="text-left">
	{#each steps as step, index (step.key)}
		{@const isLast = index === steps.length - 1}
		<li class="flex gap-3">
			<div class="flex flex-col items-center">
				<span
					class={cn(
						'grid size-5 shrink-0 place-items-center rounded-full border transition-colors duration-200 ease-out-soft',
						markerClass[step.state]
					)}
				>
					{#if step.state === 'done'}
						<svg
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="3"
							stroke-linecap="round"
							stroke-linejoin="round"
							class="size-3"
							aria-hidden="true"
						>
							<path d="M20 6 9 17l-5-5" />
						</svg>
					{:else if step.state === 'failed'}
						<svg
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="3"
							stroke-linecap="round"
							class="size-3"
							aria-hidden="true"
						>
							<path d="M18 6 6 18" />
							<path d="m6 6 12 12" />
						</svg>
					{:else}
						<span
							class={cn(
								'size-1.5 rounded-full',
								step.state === 'active' ? 'bg-primary animate-breathe' : 'bg-muted-foreground/40'
							)}
							aria-hidden="true"
						></span>
					{/if}
				</span>

				{#if !isLast}
					<!-- 连线生长：底色为轨道，前景层从顶端展开 -->
					<span class="relative mt-1 w-px flex-1 self-center bg-border" aria-hidden="true">
						<span
							class={cn(
								'absolute inset-0 origin-top bg-primary transition-transform duration-200 ease-out-soft',
								step.state === 'done' ? 'scale-y-100' : 'scale-y-0'
							)}
						></span>
					</span>
				{/if}
			</div>

			<p class={cn('pt-0.5 pb-4 text-sm', labelClass[step.state])}>
				{step.label}
			</p>
		</li>
	{/each}
</ol>

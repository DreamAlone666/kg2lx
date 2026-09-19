<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils.js';

	/**
	 * 骨架占位。
	 *
	 * 相比纯转圈 spinner，骨架在「减弱动态」偏好下被压平后仍是静态可辨识的
	 * 加载形态（能看出内容将出现在哪里），因此加载态一律优先用骨架。
	 * 呼吸由 animate-pulse 提供，全局 reduced-motion 会将其降级为静态色块。
	 */
	let { class: className, ...rest }: HTMLAttributes<HTMLDivElement> = $props();
</script>

<div class={cn('animate-pulse rounded-md bg-muted', className)} aria-hidden="true" {...rest}></div>

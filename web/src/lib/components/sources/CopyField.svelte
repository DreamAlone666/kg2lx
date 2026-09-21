<script lang="ts">
	import { Button } from '$lib/components/ui';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { toast } from '$lib/state/toast.svelte';
	import { cn } from '$lib/utils.js';

	/**
	 * 只读的值展示 + 一键复制。
	 *
	 * 复制是这个工具里用户**每次都要执行**的动作（拿到脚本链接导入洛雪音乐），
	 * 因此反馈做得完整：图标由「复制」形变为「对勾」并描边画出、按压缩放回弹、
	 * 同时弹出全局提示——用户即便视线移开也能确认结果。
	 */
	let {
		value,
		label = '脚本链接',
		class: className
	}: {
		value: string;
		label?: string;
		class?: string;
	} = $props();

	type CopyState = 'idle' | 'copied' | 'failed';

	let copyState = $state<CopyState>('idle');
	/** 每次成功复制自增；以 key 重挂载对勾图标，从而重播描边动画 */
	let drawKey = $state(0);
	let resetTimer: ReturnType<typeof setTimeout> | null = null;

	async function handleCopy() {
		const ok = await writeToClipboard(value);

		copyState = ok ? 'copied' : 'failed';
		if (ok) {
			drawKey += 1;
			toast.success('脚本链接已复制，可在洛雪音乐中作为自定义音源导入。');
		} else {
			toast.error('复制失败，请手动选中链接后复制。');
		}

		if (resetTimer) clearTimeout(resetTimer);
		resetTimer = setTimeout(() => (copyState = 'idle'), 2000);
	}

	/**
	 * Clipboard API 仅在 HTTPS 或 localhost 下可用；
	 * 本工具常见纯 HTTP 内网部署，此时必须降级到 execCommand，
	 * 否则复制会静默失败而用户毫不知情。
	 */
	async function writeToClipboard(text: string): Promise<boolean> {
		try {
			await navigator.clipboard.writeText(text);
			return true;
		} catch {
			return legacyCopy(text);
		}
	}

	function legacyCopy(text: string): boolean {
		const textarea = document.createElement('textarea');
		textarea.value = text;
		textarea.setAttribute('readonly', '');
		textarea.style.position = 'fixed';
		textarea.style.opacity = '0';
		document.body.appendChild(textarea);
		textarea.select();
		let ok = false;
		try {
			ok = document.execCommand('copy');
		} catch {
			ok = false;
		}
		textarea.remove();
		return ok;
	}

	$effect(() => {
		return () => {
			if (resetTimer) clearTimeout(resetTimer);
		};
	});
</script>

<div class={cn('flex items-center gap-2', className)}>
	<!--
		高度与右侧 sm 档按钮逐断点对齐（移动端 44px / md 以上 36px）。
		此前靠 py-2 与行高凑出一个固定的 34px，与按钮差 2px、移动端差 10px，
		在 items-center 下表现为字段比按钮上下各短一截。
		外层改成 flex 后不能再把 truncate 直接挂在它身上：flex 容器上的
		text-overflow 不作用于匿名文本项，省略号会静默失效，故交给内层 span。
	-->
	<code
		class="flex h-11 min-w-0 flex-1 items-center rounded-lg border border-border bg-muted px-3 font-mono text-xs text-muted-foreground md:h-9"
	>
		<!--
			截断的脚本地址：完整值改用 Tooltip 提示（原生 title 的延迟与样式不可控，
			键盘用户也只有在长按时才看得见）。

			触发元素是 span，所以走 `child` 渲染委托——Trigger 默认渲染成 <button>，
			直接包会在 <code> 里凭空多出一个按钮。委托后 span 由 Trigger 授予
			tabindex=0（默认值），因此这一处换来一个 tab 停：键盘用户终于能读到被截断的地址。
			`type={null}` 抹掉 Trigger 默认带的 `type="button"`（span 不接受该属性）。
			title 已删除，否则浏览器提示会与本浮层同时出现。
		-->
		<Tooltip.Root>
			<Tooltip.Trigger type={null}>
				{#snippet child({ props })}
					<span class="truncate" {...props}>{value}</span>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Content>{value}</Tooltip.Content>
		</Tooltip.Root>
	</code>

	<Button
		variant={copyState === 'failed' ? 'destructive' : 'outline'}
		size="sm"
		class="min-w-28 shrink-0"
		onclick={handleCopy}
		aria-label="{label}：复制到剪贴板"
	>
		<span class="relative size-4" aria-hidden="true">
			<svg
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
				stroke-linejoin="round"
				class="absolute inset-0 size-4 transition-[opacity,transform] duration-150 ease-out-soft {copyState ===
				'idle'
					? 'scale-100 opacity-100'
					: 'scale-50 opacity-0'}"
			>
				<rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
				<path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
			</svg>

			<svg
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
				stroke-linejoin="round"
				class="absolute inset-0 size-4 transition-opacity duration-150 ease-out-soft {copyState ===
				'idle'
					? 'opacity-0'
					: 'opacity-100'}"
			>
				{#if copyState === 'copied'}
					{#key drawKey}
						<path class="draw-check" d="M20 6 9 17l-5-5" />
					{/key}
				{:else if copyState === 'failed'}
					<path d="M18 6 6 18" />
					<path d="m6 6 12 12" />
				{/if}
			</svg>
		</span>

		{copyState === 'copied' ? '已复制' : copyState === 'failed' ? '复制失败' : '复制'}
	</Button>
</div>

<style>
	/*
	 * 对勾描边：短时间内「画」出来，比直接出现更有完成感。
	 * 全局减弱动态规则会把 animation-duration 压到 0.01ms，届时等价于直接显示。
	 */
	.draw-check {
		stroke-dasharray: 30;
		stroke-dashoffset: 30;
		animation: draw-check 320ms cubic-bezier(0.22, 1, 0.36, 1) forwards;
	}

	@keyframes draw-check {
		to {
			stroke-dashoffset: 0;
		}
	}
</style>

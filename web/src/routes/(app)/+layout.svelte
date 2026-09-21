<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminSession } from '$lib/state/admin-session.svelte';
	import { Button } from '$lib/components/ui';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { cn } from '$lib/utils.js';

	/**
	 * 应用外壳：细顶栏 + 内容区。
	 *
	 * 取代此前「页面各自写一个『← 返回音源列表』」的做法——返回逻辑分散在各页，
	 * 路径不可预测；现在导航只定义一次，页面回归为纯内容。
	 */
	let { children } = $props();

	const navItems = [
		{ href: '/sources', label: '音源' },
		{ href: '/bind', label: '绑定' }
	];

	function isActive(href: string) {
		const pathname = page.url.pathname;
		return pathname === href || pathname.startsWith(`${href}/`);
	}

	/**
	 * 兜底：只要 token 被清空（401 收口、手动登出或其它任何路径）就退出受保护区域。
	 * 这样「登出」只有一个出口，调用方不必记得自己跳转。
	 */
	$effect(() => {
		if (!adminSession.adminToken) goto('/connect');
	});
</script>

<!--
	view-transition-name 让顶栏在页面过渡中保持自身（而非参与根节点交叉淡入），
	否则常驻导航会随每次导航闪一下。
-->
<header
	class="sticky top-0 z-40 border-b border-border/70 bg-background/80 backdrop-blur-md"
	style="view-transition-name: app-header"
>
	<div class="mx-auto flex h-14 max-w-7xl items-center gap-3 px-4 sm:gap-4 sm:px-6">
		<a
			href="/sources"
			class="flex min-h-11 min-w-11 shrink-0 items-center justify-center gap-2 md:min-h-0 md:min-w-0 md:justify-start"
			aria-label="kg2lx 首页"
		>
			<span
				class="grid size-7 place-items-center rounded-lg bg-gradient-to-br from-primary to-primary-hover text-xs font-bold text-primary-foreground"
			>
				k
			</span>
			<span class="hidden text-sm font-semibold tracking-tight sm:inline">kg2lx</span>
		</a>

		<nav class="flex items-center gap-0.5" aria-label="主导航">
			{#each navItems as item (item.href)}
				{@const active = isActive(item.href)}
				<a
					href={item.href}
					aria-current={active ? 'page' : undefined}
					class={cn(
						'relative inline-flex min-h-11 items-center rounded-md px-3 text-sm font-medium transition-colors duration-150 ease-out-soft md:min-h-0 md:py-2',
						active ? 'text-foreground' : 'text-muted-foreground hover:text-foreground'
					)}
				>
					{item.label}
					<span
						class={cn(
							'absolute inset-x-3 bottom-0.5 h-0.5 origin-left rounded-full bg-primary transition-transform duration-200 ease-out-soft',
							active ? 'scale-x-100' : 'scale-x-0'
						)}
						aria-hidden="true"
					></span>
				</a>
			{/each}
		</nav>

		<div class="ml-auto flex min-w-0 items-center gap-2 sm:gap-3">
			<!--
				截断的服务器地址：完整值改用 Tooltip 提示。原生 title 的延迟、字号与位置
				都由浏览器决定，与页面里的浮层体系对不上；Tooltip 还能被键盘触发，
				title 只有长按鼠标才会出现。

				触发元素是 span 而不是按钮，所以必须走 `child` 渲染委托：Trigger 默认渲染成
				<button>，直接包会凭空多出一个语义不明的按钮，也会在顶栏多一个可聚焦控件。
				委托之后 span 由 Trigger 授予 tabindex=0（它的默认值），因此这一处换来
				一个 tab 停——这也是这一处值得换的原因（键盘可用）。

				`type={null}` 是必需的：Trigger 默认带 `type="button"`，该属性会随 `{...props}`
				摊到 span 上，而 span 不接受它（bits-ui 内部的 mergeProps 会保留 null，
				Svelte 摊开属性时把 null 当「移除该属性」）。
			-->
			<Tooltip.Root>
				<Tooltip.Trigger type={null}>
					{#snippet child({ props })}
						<span
							class="hidden max-w-56 truncate font-mono text-xs text-muted-foreground md:block"
							{...props}
						>
							{adminSession.serverBaseUrl || '当前域名'}
						</span>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>{adminSession.serverBaseUrl || '当前域名'}</Tooltip.Content>
			</Tooltip.Root>
			<Button variant="ghost" size="sm" onclick={() => adminSession.logout()}>断开连接</Button>
		</div>
	</div>
</header>

<!--
	外壳与顶栏必须同宽：两者用不同的 max-w 会让品牌与内容区的左边缘错位
	（此前专门实测过两侧左边缘一致，放宽时也要一起改）。
	5xl(1024px) 在 1440/1920 上两侧各留 200~450px，而最需要宽度的运行时日志表
	恰恰是一张列数会随数据增减的宽表（列宽按内容分配，见 LogsPanel 的 hasSongMeta）；
	7xl(1280px) 让表格与卡片网格都能用上宽屏。
-->
<main class="mx-auto w-full max-w-7xl px-4 py-6 sm:px-6 sm:py-8">
	{@render children()}
</main>

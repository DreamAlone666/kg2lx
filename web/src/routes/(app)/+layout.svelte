<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminSession } from '$lib/state/admin-session.svelte';
	import { Button } from '$lib/components/ui';
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

<header class="sticky top-0 z-40 border-b border-border/70 bg-background/80 backdrop-blur-md">
	<div class="mx-auto flex h-14 max-w-5xl items-center gap-3 px-4 sm:gap-4 sm:px-6">
		<a href="/sources" class="flex shrink-0 items-center gap-2" aria-label="kg2lx 首页">
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
						'relative rounded-md px-3 py-2 text-sm font-medium transition-colors duration-150 ease-out-soft',
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
			<span
				class="hidden max-w-56 truncate font-mono text-xs text-muted-foreground md:block"
				title={adminSession.serverBaseUrl || '当前域名'}
			>
				{adminSession.serverBaseUrl || '当前域名'}
			</span>
			<Button variant="ghost" size="sm" onclick={() => adminSession.logout()}>断开连接</Button>
		</div>
	</div>
</header>

<main class="mx-auto w-full max-w-5xl px-4 py-6 sm:px-6 sm:py-8">
	{@render children()}
</main>

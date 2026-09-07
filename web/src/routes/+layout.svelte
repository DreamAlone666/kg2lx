<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import { goto } from '$app/navigation';
	import { adminSession } from '$lib/state/admin-session.svelte';
	import { Button } from '$lib/components/ui';

	let { children } = $props();

	function handleLogout() {
		adminSession.logout();
		goto('/connect');
	}
</script>

<svelte:head>
	<title>kg2lx</title>
	<meta name="description" content="kg2lx 管理后台" />
	<link rel="icon" href={favicon} />
</svelte:head>

{#if adminSession.adminToken}
	<header class="border-b">
		<div class="mx-auto flex max-w-5xl items-center justify-between gap-2 px-4 py-2.5">
			<a href="/sources" class="text-sm font-semibold tracking-tight">kg2lx</a>
			<div class="flex min-w-0 items-center gap-3">
				<span class="truncate font-mono text-xs text-muted-foreground">
					{adminSession.serverBaseUrl || '当前域名'}
				</span>
				<Button variant="ghost" size="sm" onclick={handleLogout}>断开连接</Button>
			</div>
		</div>
	</header>
{/if}

{@render children()}

<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { Alert, Button } from '$lib/components/ui';
	import { adminSession } from '$lib/state/admin-session.svelte';

	const title = $derived(page.status === 404 ? '页面不存在' : '页面无法打开');
</script>

<svelte:head>
	<title>{title} | kg2lx</title>
</svelte:head>

<main class="mx-auto max-w-lg space-y-4 px-4 py-12">
	<h1 class="text-heading">{title}</h1>
	{#if page.status !== 404 && page.error?.message}
		<Alert>{page.error.message}</Alert>
	{/if}
	{#if adminSession.adminToken}
		<Button href={resolve('/accounts')}>返回账号管理</Button>
	{:else}
		<Button href={resolve('/connect')}>返回连接页</Button>
	{/if}
</main>

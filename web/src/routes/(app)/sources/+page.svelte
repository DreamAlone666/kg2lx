<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';

	onMount(() => {
		goto('/accounts', { replaceState: true });
	});
</script>

<svelte:head>
	<title>正在跳转 | kg2lx</title>
</svelte:head>

<p class="py-12 text-center text-sm text-muted-foreground">正在跳转到账号管理…</p>

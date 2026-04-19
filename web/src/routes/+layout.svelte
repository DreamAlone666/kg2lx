<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';

	let { children } = $props();
</script>

<svelte:head>
	<title>kg2lx</title>
	<meta name="description" content="kg2lx 管理后台" />
	<link rel="icon" href={favicon} />
</svelte:head>
{@render children()}

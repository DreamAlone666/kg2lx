<script lang="ts">
	import { onDestroy } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Alert, Button, Card, Skeleton } from '$lib/components/ui';
	import { describeApiError } from '$lib/domain';
	import { adminSession, handleAuthError } from '$lib/state/admin-session.svelte';

	let sourceId = $derived(page.params.sourceId ?? '');
	let loading = $state(true);
	let error = $state<string | null>(null);
	let abortController: AbortController | null = null;

	async function resolveAndRedirect(id: string) {
		if (abortController) abortController.abort();
		abortController = new AbortController();
		loading = true;
		error = null;

		try {
			const detail = await adminSession.api.getSource(id, abortController.signal);
			await goto(`/accounts/${detail.account_id}`, { replaceState: true });
		} catch (e: unknown) {
			if (e instanceof DOMException && e.name === 'AbortError') return;
			if (handleAuthError(e)) return;
			error = describeApiError(e);
		} finally {
			loading = false;
		}
	}

	afterNavigate(({ to }) => {
		const id = to?.params?.sourceId ?? sourceId;
		if (!id) return;
		void resolveAndRedirect(id);
	});

	onDestroy(() => {
		abortController?.abort();
	});
</script>

<svelte:head>
	<title>正在跳转 | kg2lx</title>
</svelte:head>

{#if loading && !error}
	<Card class="p-6">
		<Skeleton class="h-6 w-36" />
		<Skeleton class="mt-3 h-4 w-56" />
	</Card>
{:else if error}
	<Alert title="跳转账号详情失败">
		<p class="leading-relaxed">{error}</p>
		<Button variant="outline" size="sm" class="mt-4" href="/accounts">返回账号列表</Button>
	</Alert>
{/if}

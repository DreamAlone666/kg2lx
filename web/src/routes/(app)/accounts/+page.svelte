<script lang="ts">
	import { onMount } from 'svelte';
	import { Alert, Button, Card, Skeleton } from '$lib/components/ui';
	import AccountCard from '$lib/components/accounts/AccountCard.svelte';
	import { accountsController } from '$lib/features/accounts/controller.svelte';
	import { formatClockTime } from '$lib/domain';

	onMount(() => {
		void accountsController.loadAccounts();
	});

	const skeletonSlots = [0, 1, 2];
</script>

<svelte:head>
	<title>账号管理 | kg2lx</title>
</svelte:head>

<div class="space-y-6">
	<header class="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
		<div>
			<h1 class="text-heading tracking-tight">账号管理</h1>
			<p class="mt-1 text-sm text-muted-foreground">
				管理已绑定的酷狗概念版账号、会员权益与自动领取 VIP，并获取洛雪音乐脚本链接。
			</p>
		</div>

		<div class="flex shrink-0 items-center gap-2">
			{#if accountsController.lastLoadedAt}
				<span class="tabular hidden text-xs whitespace-nowrap text-muted-foreground sm:inline">
					列表更新于 {formatClockTime(accountsController.lastLoadedAt)}
				</span>
			{/if}
			<Button
				variant="outline"
				pending={accountsController.loadingList}
				pendingLabel="刷新中…"
				onclick={() => accountsController.loadAccounts()}
			>
				刷新列表
			</Button>
			{#if accountsController.loadingList || accountsController.accounts.length > 0 || accountsController.error}
				<Button href="/bind">绑定账号</Button>
			{/if}
		</div>
	</header>

	{#if accountsController.loadingList && accountsController.accounts.length === 0}
		<div class="grid gap-4 sm:grid-cols-[repeat(auto-fit,minmax(18rem,24rem))] sm:justify-start">
			{#each skeletonSlots as slot (slot)}
				<Card class="p-6">
					<Skeleton class="h-5 w-20" />
					<Skeleton class="mt-4 h-6 w-36" />
					<Skeleton class="mt-4 h-4 w-24" />
					<Skeleton class="mt-3 h-3 w-40" />
					<Skeleton class="mt-6 h-8 w-full" />
				</Card>
			{/each}
		</div>
	{:else if accountsController.accounts.length === 0}
		{#if accountsController.error}
			<Alert title="加载账号列表失败">
				<p class="leading-relaxed">{accountsController.error}</p>
				<Button
					variant="outline"
					size="sm"
					class="mt-4"
					onclick={() => accountsController.loadAccounts()}
				>
					重试
				</Button>
			</Alert>
		{:else}
			<Card class="flex flex-col items-center justify-center gap-1 border-dashed px-6 py-16 text-center">
				<p class="font-medium">还没有绑定任何账号</p>
				<p class="mt-1 max-w-sm text-sm leading-relaxed text-muted-foreground">
					绑定酷狗概念版账号后，这里会出现账号权益状态与可直接导入洛雪音乐的脚本链接。
				</p>
				<Button href="/bind" class="mt-5">绑定第一个账号</Button>
			</Card>
		{/if}
	{:else}
		{#if accountsController.error}
			<Alert>
				<div class="flex items-start justify-between gap-4">
					<p class="leading-relaxed">{accountsController.error}</p>
					<Button variant="ghost" size="sm" onclick={() => accountsController.clearError()}>
						关闭
					</Button>
				</div>
			</Alert>
		{/if}

		<div class="grid gap-4 sm:grid-cols-[repeat(auto-fit,minmax(18rem,24rem))] sm:justify-start">
			{#each accountsController.accounts as account (account.account_id)}
				<AccountCard {account} />
			{/each}
		</div>
	{/if}
</div>

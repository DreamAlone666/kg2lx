<script lang="ts">
	import { onMount } from 'svelte';
	import ConnectForm from '$lib/components/connect/ConnectForm.svelte';
	import { connectController } from '$lib/features/connect/controller.svelte';

	// 控制器是单例，重新进入本页时把上一次的凭证与错误清掉
	onMount(() => connectController.reset());
</script>

<svelte:head>
	<title>连接后端 | kg2lx</title>
</svelte:head>

<div
	class="relative flex min-h-screen flex-col items-center justify-center overflow-hidden px-4 py-12"
>
	<!-- 装饰性光晕：给单屏页面一个视觉落点，不承载任何信息 -->
	<div
		class="pointer-events-none absolute inset-x-0 top-0 h-80 bg-[radial-gradient(50%_60%_at_50%_0%,var(--color-primary-soft),transparent_70%)]"
		aria-hidden="true"
	></div>

	<div class="relative w-full max-w-md">
		<div class="mb-7 text-center">
			<span
				class="mx-auto mb-4 grid size-11 place-items-center rounded-xl bg-gradient-to-br from-primary to-primary-hover text-base font-bold text-primary-foreground shadow-soft"
			>
				k
			</span>
			<h1 class="text-heading tracking-tight">连接至后端</h1>
			<p class="mt-2 text-sm text-muted-foreground">
				输入服务器地址与管理员 Token，开始管理你的音源。
			</p>
		</div>

		<ConnectForm />
	</div>
</div>

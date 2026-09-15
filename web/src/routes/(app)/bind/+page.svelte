<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import QrLoginPanel from '$lib/components/qr-login/QrLoginPanel.svelte';
	import { qrLoginController } from '$lib/features/qr-login/controller.svelte';

	/** 绑定成功后停留片刻再跳转，让用户来得及看清结果。 */
	const REDIRECT_DELAY_MS = 3000;

	let redirectTimer: ReturnType<typeof setTimeout> | null = null;

	// 认证守卫已上提到 (app)/+layout.ts，页面只负责会话恢复与 URL 同步
	onMount(() => {
		const sessionId = page.url.searchParams.get('session_id');
		if (sessionId) {
			qrLoginController.resume(sessionId);
		} else {
			// 控制器是单例，上一次绑定的终态需显式清理
			qrLoginController.reset();
		}
	});

	onDestroy(() => {
		qrLoginController.stop();
		clearRedirectTimer();
	});

	async function startNewLogin() {
		await qrLoginController.start();
		if (!qrLoginController.sessionId) return;

		// 会话写进 URL，刷新页面后仍能续接同一个会话
		const url = new URL(window.location.href);
		url.searchParams.set('session_id', qrLoginController.sessionId);
		window.history.replaceState({}, '', url);
	}

	function clearRedirectTimer() {
		if (redirectTimer !== null) {
			clearTimeout(redirectTimer);
			redirectTimer = null;
		}
	}

	/**
	 * 会话进入终态时收尾：清掉 URL 里的 session_id（避免刷新后去轮询一个已结束的会话），
	 * 绑定成功则稍后跳转到音源详情。
	 */
	$effect(() => {
		if (!qrLoginController.isTerminal) return;

		const url = new URL(window.location.href);
		if (url.searchParams.has('session_id')) {
			url.searchParams.delete('session_id');
			window.history.replaceState({}, '', url);
		}

		clearRedirectTimer();

		const bound = qrLoginController.boundSource;
		if (bound) {
			const sourceId = bound.source.source_id;
			redirectTimer = setTimeout(() => goto(`/sources/${sourceId}`), REDIRECT_DELAY_MS);
		}

		return clearRedirectTimer;
	});
</script>

<svelte:head>
	<title>绑定酷狗账号 | kg2lx</title>
</svelte:head>

<div class="mx-auto max-w-md">
	<QrLoginPanel onRestart={startNewLogin} />
</div>

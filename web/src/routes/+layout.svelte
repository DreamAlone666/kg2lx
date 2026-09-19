<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import { onNavigate } from '$app/navigation';
	import { Toaster } from '$lib/components/ui';
	import { prefersReducedMotion } from '$lib/utils/motion';

	/**
	 * 根壳：只负责三件事——引入令牌层、挂载全局提示宿主、接入视图过渡。
	 * 顶栏与认证守卫下沉到 (app) 路由组，因为连接页不属于受保护区域、也不该有导航。
	 */
	let { children } = $props();

	type ViewTransitionDocument = Document & {
		startViewTransition?: (callback: () => void | Promise<void>) => { finished: Promise<void> };
	};

	/**
	 * 列表 ↔ 详情之间使用共享元素过渡，让卡片「生长」为详情页。
	 * - 必须能力检测：Safari 与部分 Firefox 版本尚未支持
	 * - 必须等待 navigation.complete：否则过渡会在数据就绪前被提前结束而闪烁
	 * - 必须尊重减弱动态：CSS 降级覆盖不到 API 驱动的过渡
	 */
	onNavigate((navigation) => {
		const startViewTransition = (document as ViewTransitionDocument).startViewTransition?.bind(
			document
		);
		if (!startViewTransition) return;
		if (prefersReducedMotion()) return;

		return new Promise<void>((resolve) => {
			startViewTransition(async () => {
				resolve();
				await navigation.complete;
			});
		});
	});
</script>

<svelte:head>
	<title>kg2lx</title>
	<meta name="description" content="kg2lx 管理后台" />
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}
<Toaster />

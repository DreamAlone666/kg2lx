import { browser } from '$app/environment';

/**
 * 响应式读取页面可见性。
 *
 * 让界面诚实地反映后台行为：页面被隐藏时轮询会暂停，指示器应同步从「实时」
 * 切换为「已暂停」，而不是继续假装在刷新。
 *
 * 需在组件内调用（内部使用 $effect 绑定 visibilitychange 监听）。
 */
export function usePageVisible(): { readonly visible: boolean } {
	let visible = $state(browser ? !document.hidden : true);

	$effect(() => {
		if (!browser) return;

		const sync = () => {
			visible = !document.hidden;
		};
		sync();
		document.addEventListener('visibilitychange', sync);
		return () => document.removeEventListener('visibilitychange', sync);
	});

	return {
		get visible() {
			return visible;
		},
	};
}

import { browser } from '$app/environment';

/**
 * 响应式读取系统「减弱动态」偏好。
 *
 * layout.css 已做全局 CSS 降级，但 CSS 覆盖不到 JS 驱动的过渡：Svelte transition、
 * View Transitions 与帧动画都必须在 JS 侧单独判断，否则会在用户明确要求减少动效时
 * 依然播放。
 *
 * 需在组件内调用（内部使用 $effect 订阅媒体查询变化）。
 */
export function useReducedMotion(): { readonly reduced: boolean } {
	const query = browser ? window.matchMedia('(prefers-reduced-motion: reduce)') : null;
	let reduced = $state(query?.matches ?? false);

	$effect(() => {
		if (!query) return;

		const sync = () => {
			reduced = query.matches;
		};
		sync();
		query.addEventListener('change', sync);
		return () => query.removeEventListener('change', sync);
	});

	return {
		get reduced() {
			return reduced;
		},
	};
}

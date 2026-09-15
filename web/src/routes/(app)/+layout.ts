import { redirect } from '@sveltejs/kit';
import { adminSession } from '$lib/state/admin-session.svelte';

/**
 * 受保护路由组的统一认证守卫。
 *
 * 此前 sources / sources/[sourceId] / bind 三个页面各自在 onMount 里写一遍
 * `if (!adminSession.adminToken) goto('/connect')`，新增页面时极易漏写；
 * 提到布局层后，只要路由落在 (app) 组内就自动受保护。
 *
 * 由于根布局已设 ssr=false，此处只在浏览器端执行，可直接读取会话单例。
 * 登出时的跳转由 (app)/+layout.svelte 的兜底 effect 负责，避免两处各写一次。
 */
export function load() {
	if (!adminSession.adminToken) {
		redirect(307, '/connect');
	}
}

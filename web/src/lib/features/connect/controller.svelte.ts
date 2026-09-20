import { goto } from '$app/navigation';
import { AdminApi } from '$lib/api/admin';
import { AdminApiError } from '$lib/api/errors';
import { HttpClient } from '$lib/api/http';
import { adminSession } from '$lib/state/admin-session.svelte';

/** 连接校验超时。内网直连远快于此，超时基本等同于地址不可达。 */
const CONNECT_TIMEOUT_MS = 8000;

/**
 * 连接流程控制器。
 *
 * 与 sources / qr-login / source-logs 保持同一形态（模块级单例 + $state），
 * 使「网络调用」始终只出现在 features 层，组件与页面只消费状态。
 *
 * 注意：因为是单例，登录后返回本页会残留上一次的 Token。
 * 页面挂载时调用 reset() 重新从会话读取，避免把过期凭证留在内存里。
 */
class ConnectController {
	baseUrl = $state(adminSession.serverBaseUrl || '');
	token = $state(adminSession.adminToken);
	loading = $state(false);
	error = $state<string | null>(null);

	/** 从会话恢复表单初值，并清空上一次的错误。 */
	reset() {
		this.baseUrl = adminSession.serverBaseUrl || '';
		this.token = adminSession.adminToken;
		this.loading = false;
		this.error = null;
	}

	async connect() {
		this.loading = true;
		this.error = null;

		try {
			const serverBaseUrl = this.baseUrl.trim().replace(/\/$/, '');
			const adminToken = this.token.trim();
			const api = new AdminApi(new HttpClient({ baseUrl: serverBaseUrl, adminToken }));
			const signal = AbortSignal.timeout(CONNECT_TIMEOUT_MS);

			// 先用 healthz 探连通性，再用 listSources 验证 Token 确实有效，
			// 避免把「地址可达但凭证错误」误判为连接成功。
			await api.healthz(signal);
			await api.listSources(signal);

			adminSession.setSession(serverBaseUrl, adminToken);
			await goto('/sources');
		} catch (e) {
			this.error = describeConnectError(e);
		} finally {
			this.loading = false;
		}
	}
}

/**
 * 把底层异常翻译成用户能直接行动的中文提示。
 *
 * 三类失败的下一步动作完全不同（检查服务 / 核对凭证 / 排查网络与跨域），
 * 因此必须区分；fetch 网络层失败统一抛 TypeError，原始信息
 * （如 "Failed to fetch"）对用户没有意义，不能直接抛给界面。
 */
function describeConnectError(e: unknown): string {
	if (e instanceof DOMException && (e.name === 'TimeoutError' || e.name === 'AbortError')) {
		return '连接超时：请确认后端服务正在运行，且该地址能从浏览器访问。';
	}
	if (e instanceof AdminApiError && e.status === 401) {
		return '管理员 Token 不正确（401）：请核对部署时设置的 ADMIN_TOKEN。';
	}
	// 没有 code 说明响应体不是本仓的错误信封，有两种成因，下一步动作完全不同：
	// 5xx 是中间层在目标不可达时代回的（开发期由 Vite 代理转发，连不上后端即 502），
	// 该去把后端起起来；2xx/4xx 则是这个地址上放着的是一份网页，该改地址。
	if (e instanceof AdminApiError && !e.code) {
		return e.status >= 500
			? `后端未响应（HTTP ${e.status}）：请确认 kg2lx 服务已启动，且该地址可达。`
			: `服务器返回的不是接口数据（HTTP ${e.status}）：请确认该地址指向 kg2lx 后端本身；留空表示使用当前域名。`;
	}
	if (e instanceof TypeError) {
		return '无法连接到服务器：请确认地址以 http(s):// 开头、后端已启动，且浏览器可以访问（跨域或网络问题也会导致此错误）。';
	}
	return (e as Error)?.message || '连接失败';
}

export const connectController = new ConnectController();

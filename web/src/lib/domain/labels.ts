import type {
	AccountStatus,
	LoginSessionStatus,
	ProviderKind,
	RuntimeLogListItem,
} from '$lib/api/types';

/**
 * 领域词典 —— 业务状态到中文标签的**唯一事实来源**。
 *
 * 页面只负责展示，不负责「领域翻译」。此前 providerMap / statusMap / accountStatusMap
 * / stageMap 各自在路由与组件里重复定义，改一处文案需要改多处，且容易不一致；
 * 集中到此处后，组件层不再认识业务枚举字符串。
 *
 * 注意：新增枚举值时必须同步更新对应表，否则回退到原值显示（见 labelOf）。
 */

export const providerLabel: Record<ProviderKind, string> = {
	kugou_lite: '酷狗概念版',
};

export const accountStatusLabel: Record<AccountStatus, string> = {
	active: '正常',
	pending: '待处理',
	expired: '已过期',
	disabled: '已禁用',
	login_failed: '登录失败',
};

export const sessionStatusLabel: Record<LoginSessionStatus, string> = {
	pending: '准备中',
	waiting_scan: '等待扫码',
	waiting_confirm: '等待确认',
	authorized: '已授权',
	bound: '绑定成功',
	expired: '已过期',
	failed: '失败',
};

export const runtimeStageLabel: Record<RuntimeLogListItem['stage'], string> = {
	precheck: '预检',
	ensure_dfid: 'DFID',
	refresh_login: '刷新登录',
	fetch_music_url: '取链',
};

/**
 * 安全的查表：后端可能先于前端新增枚举值，
 * 此时回退到原始字符串而不是渲染空白，便于用户反馈问题。
 */
export function labelOf<T extends string>(
	map: Record<T, string>,
	key: string | null | undefined,
	fallback = '未知'
): string {
	if (!key) return fallback;
	return (map as Record<string, string>)[key] ?? key;
}

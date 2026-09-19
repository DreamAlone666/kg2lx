import type { AccountStatus, LoginSessionStatus } from '$lib/api/types';

/**
 * 语义色调 —— 业务状态到状态色的**唯一映射**。
 *
 * 组件层不自行判断业务状态落在哪一档，只消费 Tone；这样「同一状态在任何页面
 * 必须是同一个颜色」才能被强制保证，颜色才成为信息而不是噪音。
 *
 * Tone 对应的色值定义在 routes/layout.css 的 @theme 中：
 *   success 绿「正常 / 成功」  warning 琥珀「警告 / 待处理」
 *   danger  红「错误 / 失败」  info    蓝「进行中」   neutral 灰「空闲 / 未知」
 */

export type Tone = 'success' | 'warning' | 'danger' | 'info' | 'neutral';

const accountStatusTones: Record<AccountStatus, Tone> = {
	active: 'success',
	pending: 'warning',
	expired: 'danger',
	disabled: 'neutral',
	login_failed: 'danger',
};

/** 账号状态：过期与登录失败都是需要用户介入的故障，故同为 danger。 */
export function accountStatusTone(status: AccountStatus | string | null | undefined): Tone {
	return accountStatusTones[status as AccountStatus] ?? 'neutral';
}

const sessionStatusTones: Record<LoginSessionStatus, Tone> = {
	pending: 'neutral',
	waiting_scan: 'info',
	waiting_confirm: 'info',
	authorized: 'info',
	bound: 'success',
	// 二维码过期是一次可重试的超时，不是故障，故用 warning 而非 danger；
	// 这样与账号状态里的 expired(danger) 区分开，避免用户误以为账号出了问题。
	expired: 'warning',
	failed: 'danger',
};

export function sessionStatusTone(status: LoginSessionStatus | string | null | undefined): Tone {
	return sessionStatusTones[status as LoginSessionStatus] ?? 'neutral';
}

/** 运行时请求结果：成功 / 失败。 */
export function requestOutcomeTone(ok: boolean): Tone {
	return ok ? 'success' : 'danger';
}

/**
 * VIP 标记。使用 warning 档暖琥珀，替代原先硬编码的 bg-yellow-500，
 * 同时把「会员金色」的观感保留下来。未激活时不渲染标记，故不存在歧义。
 */
export function vipTone(vipActive: boolean): Tone {
	return vipActive ? 'warning' : 'neutral';
}

/**
 * 音质。音质属于「档位」而非「状态」，本不该占用状态色；这里映射到 Tone 是为了
 * 复用状态胶囊的柔和底样式，取值刻意限制在 neutral → info → success 的强度递进，
 * 不使用 danger / warning，避免与真正的失败状态抢语义。
 */
export function qualityTone(quality: string | null | undefined): Tone {
	const q = (quality ?? '').toLowerCase();
	if (!q) return 'neutral';
	if (q.includes('flac') || q.includes('lossless') || q.includes('hires')) return 'success';
	if (q.includes('320')) return 'info';
	return 'neutral';
}

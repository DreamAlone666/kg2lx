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
 * 会员权益标记的色调。
 *
 * 用 warning 档暖琥珀，替代更早硬编码的 bg-yellow-500，把「会员金色」的观感保留下来。
 * 它标记的是「这一项是会员权益」，**不表达在期与否**——在期与否由「这一条有没有被渲染
 * 出来」表达（调用方按 active 过滤），到期是否临近由 entitlementExpiringTone 表达。
 * 因此这里刻意**不带参数**：一个不携带状态的标签，颜色不该随状态变，否则同一个胶囊
 * 会同时声称两件事。
 *
 * 留在 tone.ts 而不是写进组件，是因为它是「会员权益 → 色调」这条业务映射的定义处，
 * 与上面几张表同一分工。
 */
export const VIP_ENTITLEMENT_TONE: Tone = 'warning';

/**
 * 权益「临近到期」的判定窗口（天）。
 *
 * 定义在领域层而不是组件里：阈值与色调是同一个判断的两半（「什么时候该提醒」与
 * 「提醒时用什么颜色」），散到路由后调窗口就得去翻页面。取 7 天是因为续费是个需要
 * 提前量的动作；再往前放宽，常年有效的权益会一直挂着警示胶囊，状态色就退化成装饰。
 */
export const ENTITLEMENT_EXPIRING_DAYS = 7;

/**
 * 单条会员权益的「临近到期」标记；不需要提醒时返回 **null**（不渲染状态胶囊）。
 *
 * 与返回 neutral 不是一回事：neutral 是一个真实取值（「空闲 / 未知」），画出来会让
 * 一条正常在期的权益看起来带着某种待处理状态。返回 null 表达的是「无事可说」，
 * Tone 因此不必新增成员，保持闭集。
 *
 * 只有一种情况给出 warning：**有到期时间、且剩余时间落在窗口内**。没有到期时间就
 * 无从谈「临近」；到期时间已过也不贴胶囊——理由见下。
 *
 * 为什么会同时看到「`active` 为真」与「`end_at` 已过」：这对字段来自**落盘快照**，
 * 不是每次请求现算的——凡是一次「拿到上游新快照」的写入都会整体重写它（取链时的懒刷新、
 * 后台点「刷新」、重新扫码绑定都属于这一类），而这里**没有后台定时循环**，闲置账号的快照
 * 可以无限期停在很久以前那一份。所以它如实表达的是「上次刷新时上游说的样子」，两个字段
 * 对不上不表示数据坏了。而「即将到期」是对**未来**的断言，对已经过去的时刻说这句话就是
 * 假的，故返回 null——本段的诚实边界是「不呈现推算结果，只呈现快照」。
 *
 * 反过来说，「有 end_at」也不等于在期（快照变旧会给出 `active=false` 而 `end_at` 仍在
 * 未来的组合）。因此本函数只负责时间维度，在期与否由调用方按 `active` 过滤，两者不可互换。
 *
 * 色调取 warning（「警告 / 待处理」）：权益到期后账号一切正常、仅部分歌取不到高音质，
 * 是**提前处理**的提醒而不是已发生的故障，故不用 danger（与 sessionStatusTone 里
 * 「二维码过期」判成 warning 同一条理由）。
 *
 * `now` 可注入只为便于离线核算边界，调用方按默认取当前时刻即可（同 formatLogTime）。
 */
export function entitlementExpiringTone(
	endAt: number | null | undefined,
	now: number = Date.now()
): Tone | null {
	if (endAt === null || endAt === undefined || !Number.isFinite(endAt)) return null;
	const remainingMs = endAt * 1000 - now;
	if (remainingMs <= 0) return null;
	return remainingMs <= ENTITLEMENT_EXPIRING_DAYS * 24 * 60 * 60 * 1000 ? 'warning' : null;
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

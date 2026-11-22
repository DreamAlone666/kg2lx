import type {
	EntitlementAction,
	EntitlementEventDetail,
	RenewalOutcome,
	RenewalReason,
	RenewalState,
	RenewalStatus,
	StepState,
} from '$lib/api/types';
import { formatDateTime } from './format';
import type { Tone } from './tone';

/**
 * 每日领升（Renewal）领域词典与状态推演 —— 唯一事实来源。
 *
 * 集中收敛后端 RenewalState（9 态）、StepState（3 态）、RenewalReason（7 因）、
 * RenewalOutcome（5 态）、EntitlementAction（4 动作），以及：
 *   - 实时倒计时目标时刻提取
 *   - 手动触发按钮文案与禁用判断
 *   - POST /renewal/claim 200 回执的诚实反馈文案
 *   - 账号卡片上的克制一行提示
 */

export const renewalStateLabel: Record<RenewalState, string> = {
	ready: '待领升',
	waiting_for_reconciliation: '待核对日记录',
	verification_paused: '今日已暂停',
	upstream_unavailable: '上游暂不可用',
	rate_limited: '限流冷却中',
	propagating: '生效等待中',
	awaiting_upgrade: '待升级概念版VIP',
	completed: '今日已完成',
	attempts_exhausted: '今日重试已用尽',
};

export const renewalStepLabel: Record<StepState, string> = {
	ready: '待执行',
	uncertain: '待核对',
	done: '已完成',
};

export const renewalReasonLabel: Record<RenewalReason, string> = {
	manual: '手动触发',
	t1_low_battery: '余量不足预领 (T1)',
	t2_expired_refusal: '过期遇拒补领 (T2)',
	confirm_snapshot: '确认权益快照',
	reconcile_mismatch: '权益矛盾对账',
	recover_write: '继续完成领升',
	post_refresh: '刷新后后台同步',
};

export const renewalOutcomeLabel: Record<RenewalOutcome, string> = {
	succeeded: '成功',
	already_done: '今日已完成',
	failed: '失败',
	uncertain: '结果待核对',
	deferred: '已延后',
};

export const entitlementActionLabel: Record<EntitlementAction, string> = {
	sync: '同步快照',
	claim: '领取畅听VIP',
	upgrade: '升级概念版VIP',
	reconcile_record: '核对日记录',
};

/** 131001 / 297002 为上游「今日已领/已升级」幂等返回码，不应误作错误。 */
export function isEntitlementAlreadyDone(detail: EntitlementEventDetail): boolean {
	return (
		detail.outcome === 'already_done' ||
		detail.upstream_code === 131001 ||
		detail.upstream_code === 297002
	);
}

export interface RenewalDeadlineTarget {
	label: string;
	until: number;
}

export interface RenewalDeadline extends RenewalDeadlineTarget {
	remainingSeconds: number;
}

/**
 * 当前状态对应的截止时刻（Unix 秒），供倒计时展示与到点自动刷新共用同一真源。
 *
 * 与后端 domain/renewal.rs 严格对齐：
 * - propagating -> propagating_until（升级后 ~120s 播放生效窗）
 * - verification_paused / upstream_unavailable -> paused_until（日级暂停至下一个上游日）
 * - rate_limited -> rate_limit_until（限流退避窗）
 * - waiting_for_reconciliation / awaiting_upgrade / ready -> retry_at（短冷却 / 切日窗口）
 */
export function renewalDeadlineTarget(renewal: RenewalStatus): RenewalDeadlineTarget | null {
	let label: string | null = null;
	let until: number | null = null;

	switch (renewal.state) {
		case 'propagating':
			label = '预计播放生效倒计时';
			until = renewal.propagating_until;
			break;
		case 'verification_paused':
		case 'upstream_unavailable':
			label = '暂停恢复倒计时';
			until = renewal.paused_until;
			break;
		case 'rate_limited':
			label = '限流冷却倒计时';
			until = renewal.rate_limit_until;
			break;
		case 'waiting_for_reconciliation':
			label = '下次可核对倒计时';
			until = renewal.retry_at;
			break;
		case 'awaiting_upgrade':
		case 'ready':
			label = '下次可重试倒计时';
			until = renewal.retry_at;
			break;
		default:
			return null;
	}

	if (until === null || !Number.isFinite(until)) return null;
	return { label, until };
}

/**
 * 提取当前状态下仍在未来的倒计时截止时刻；已到期或无截止时刻时返回 null。
 */
export function activeRenewalDeadline(
	renewal: RenewalStatus,
	nowSec: number = Math.floor(Date.now() / 1000)
): RenewalDeadline | null {
	const target = renewalDeadlineTarget(renewal);
	if (!target || target.until <= nowSec) return null;

	return {
		...target,
		remainingSeconds: Math.max(0, target.until - nowSec),
	};
}

/** 根据当前状态与倒计时生成克制、准确的人话说明，异常态附带明确下一步动作。 */
export function describeRenewalState(
	renewal: RenewalStatus,
	nowSec: number = Math.floor(Date.now() / 1000)
): string {
	switch (renewal.state) {
		case 'ready':
			if (renewal.retry_at && renewal.retry_at > nowSec) {
				return '处于暂缓窗口（如切日前留余或读取退避），冷却结束后可继续。';
			}
			if (renewal.svip_confirmation_required) {
				return renewal.enabled
					? '尚未确认概念版VIP 有效期，点击上方「同步权益」或在播放时自动确认后生效。'
					: '尚未确认概念版VIP 有效期；自动开关已关闭，可先「同步权益」或按需手动领取。';
			}
			return renewal.enabled
				? '今日尚未领升，将在高品播放且概念版VIP 剩余不足 48 小时或过期遇拒时自动执行。'
				: '今日尚未领升；自动开关已关闭，仍可按需手动领取。';
		case 'waiting_for_reconciliation':
			return '上次写入未收到明确回执，需先核对上游日记录确认进度，避免重复提交。';
		case 'verification_paused':
			return renewal.paused_until && renewal.paused_until > nowSec
				? '上游触发人机验证，领升已暂停至 ' +
						formatDateTime(renewal.paused_until) +
						'。如需立即续期，可在酷狗概念版 App 手动领取后点击上方「同步权益」。'
				: '上游触发人机验证，今日领升已暂停；可在酷狗概念版 App 手动领取后点击上方「同步权益」。';
		case 'upstream_unavailable':
			return renewal.paused_until && renewal.paused_until > nowSec
				? '上游网关返回不明异常，为避免异常重试已暂停至 ' +
						formatDateTime(renewal.paused_until) +
						'。可查看事件日志或在 App 手动领取后点击上方「同步权益」。'
				: '上游网关返回不明异常，今日领升已暂停；可查看事件日志或在 App 手动领取后同步。';
		case 'rate_limited':
			return renewal.rate_limit_until && renewal.rate_limit_until > nowSec
				? '上游接口触发频率限制，请等待冷却结束。'
				: '限流冷却窗口已过，可再次尝试或同步权益查看状态。';
		case 'propagating':
			return '今日概念版VIP 已升级，上游播放约需 2 分钟生效；期间高品歌曲可能暂不可用。';
		case 'awaiting_upgrade':
			return '今日已领取畅听VIP，升级概念版VIP 尚未完成，可手动点击「继续升级」或在下次播放时自动续跑。';
		case 'completed':
			return renewal.svip_confirmation_required
				? '今日畅听VIP 领取与概念版VIP 升级均已完成，可点击上方「同步权益」确认上游快照。'
				: '今日畅听VIP 领取与概念版VIP 升级均已完成。';
		case 'attempts_exhausted':
			return '今日超时重试次数已用尽，可稍后在酷狗概念版 App 手动领取并点击「同步权益」，或等待下一个上游日（UTC+8）。';
	}
}

export interface RenewalActionControl {
	label: string;
	disabled: boolean;
}

/** 计算手动领升按钮的准确文案与可用态。 */
export function renewalActionControl(
	renewal: RenewalStatus,
	nowSec: number = Math.floor(Date.now() / 1000)
): RenewalActionControl {
	const retryActive = renewal.retry_at !== null && renewal.retry_at > nowSec;

	switch (renewal.state) {
		case 'ready':
			return retryActive
				? { label: '等待重试冷却', disabled: true }
				: { label: '领取今日 VIP', disabled: false };
		case 'awaiting_upgrade':
			return retryActive
				? { label: '等待重试冷却', disabled: true }
				: { label: '继续升级', disabled: false };
		case 'waiting_for_reconciliation':
			return retryActive
				? { label: '等待核对冷却', disabled: true }
				: { label: '核对日记录', disabled: false };
		case 'propagating':
			return renewal.propagating_until !== null && renewal.propagating_until <= nowSec
				? { label: '今日已完成', disabled: true }
				: { label: '等待生效中', disabled: true };
		case 'completed':
			return { label: '今日已完成', disabled: true };
		case 'verification_paused':
		case 'upstream_unavailable':
			if (renewal.paused_until !== null && renewal.paused_until <= nowSec) {
				return { label: '领取今日 VIP', disabled: false };
			}
			return { label: '今日已暂停', disabled: true };
		case 'attempts_exhausted':
			return { label: '今日重试已用尽', disabled: true };
		case 'rate_limited':
			if (renewal.rate_limit_until !== null && renewal.rate_limit_until <= nowSec) {
				if (renewal.claim === 'uncertain' || renewal.upgrade === 'uncertain') {
					return { label: '核对日记录', disabled: false };
				}
				return {
					label: renewal.claim === 'done' && renewal.upgrade !== 'done' ? '继续升级' : '领取今日 VIP',
					disabled: false,
				};
			}
			return { label: '限流冷却中', disabled: true };
	}
}

/**
 * 诚实解释 POST .../renewal/claim 返回的 RenewalStatus（HTTP 几乎总是 200）。
 * 绝不说出比 state / last_outcome 更强的成功断言。
 */
export function describeClaimResult(status: RenewalStatus): { tone: Tone; message: string } {
	switch (status.state) {
		case 'completed':
			if (status.last_outcome === 'already_done') {
				return { tone: 'success', message: '今日权益已领取并升级，无需重复操作。' };
			}
			return {
				tone: 'success',
				message: status.svip_confirmation_required
					? '今日领升步骤已完成，可点击「同步权益」确认概念版VIP 快照。'
					: '今日领升已完成。',
			};
		case 'propagating':
			return {
				tone: 'info',
				message: '今日概念版VIP 已升级，上游播放约需 2 分钟生效。',
			};
		case 'awaiting_upgrade':
			if (status.last_outcome === 'failed') {
				return {
					tone: 'warning',
					message: '畅听VIP已领取，但本次升级概念版VIP未成功，请查看事件日志。',
				};
			}
			if (status.last_outcome === 'deferred') {
				return {
					tone: 'info',
					message: '畅听VIP已就绪，升级概念版VIP已延后，请稍后再试。',
				};
			}
			return {
				tone: 'info',
				message: '畅听VIP已领取，可点击「继续升级」完成概念版VIP升级。',
			};
		case 'waiting_for_reconciliation':
			return {
				tone: 'warning',
				message: '上游返回状态不明确，已进入待核对状态，请核对日记录或查看日志。',
			};
		case 'verification_paused':
			return {
				tone: 'danger',
				message: '上游要求安全或人机校验，今日自动领升已暂停。',
			};
		case 'rate_limited':
			return {
				tone: 'warning',
				message: '触发上游频率限制，请等待冷却结束后再试。',
			};
		case 'upstream_unavailable':
			return {
				tone: 'warning',
				message: '上游接口暂不可用，请稍后重试。',
			};
		case 'attempts_exhausted':
			return {
				tone: 'danger',
				message: '今日重试次数已用尽，请查看事件日志排查原因。',
			};
		case 'ready':
			if (status.last_outcome === 'failed') {
				return { tone: 'warning', message: '本次领升未成功，请查看事件日志。' };
			}
			if (status.last_outcome === 'uncertain') {
				return { tone: 'warning', message: '操作结果待核对，请查看事件日志。' };
			}
			if (status.last_outcome === 'deferred') {
				return { tone: 'info', message: '操作已延后处理，请稍后再试。' };
			}
			if (status.last_outcome === 'already_done') {
				return { tone: 'success', message: '今日相关步骤已处理。' };
			}
			return { tone: 'info', message: '领升状态已更新。' };
	}
}

export interface CardRenewalHint {
	tone: Tone | 'muted';
	text: string;
}

/** 账号卡片上的克制一行领升提示。 */
export function accountCardRenewalHint(renewal: RenewalStatus): CardRenewalHint {
	switch (renewal.state) {
		case 'propagating':
			return { tone: 'info', text: '领升生效等待中' };
		case 'waiting_for_reconciliation':
			return { tone: 'warning', text: '领升待核对日记录' };
		case 'verification_paused':
			return { tone: 'danger', text: '每日领升今日已暂停' };
		case 'upstream_unavailable':
			return { tone: 'warning', text: '领升上游暂不可用' };
		case 'rate_limited':
			return { tone: 'warning', text: '领升限流冷却中' };
		case 'awaiting_upgrade':
			return { tone: 'warning', text: '待升级概念版VIP' };
		case 'attempts_exhausted':
			return { tone: 'danger', text: '今日领升重试已用尽' };
		case 'ready':
		case 'completed':
			return renewal.enabled
				? { tone: 'muted', text: '每日自动领升已开启' }
				: { tone: 'muted', text: '每日自动领升未开启' };
	}
}

import type {
	EntitlementAction,
	EntitlementEventDetail,
	RenewalOutcome,
	RenewalReason,
	RenewalState,
	RenewalStatus,
	StepState,
} from '$lib/api/types';
import type { Tone } from './tone';

/**
 * 自动领取 VIP（Renewal）领域词典与状态推演 —— 唯一事实来源。
 *
 * 集中收敛后端 RenewalState、StepState（3 态）、RenewalReason（7 因）、
 * RenewalOutcome（5 态）、EntitlementAction（4 动作），以及：
 *   - 实时倒计时目标时刻提取
 *   - 手动触发按钮文案与禁用判断
 *   - POST /renewal/claim 200 回执的诚实反馈文案
 *   - 账号卡片上的克制一行提示
 */

export const renewalStateLabel: Record<RenewalState, string> = {
	ready: '今日未领取',
	waiting_for_reconciliation: '待核对记录',
	reconciliation_exhausted: '今日核对次数已用尽',
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
	t1_low_battery: '有效期不足自动领取',
	t2_expired_refusal: '过期播歌受阻补领',
	confirm_snapshot: '确认会员快照',
	reconcile_mismatch: '状态不一致核对',
	recover_write: '继续完成升级',
	post_refresh: '刷新后同步',
};

export const renewalOutcomeLabel: Record<RenewalOutcome, string> = {
	succeeded: '成功',
	already_done: '今日已完成',
	failed: '失败',
	uncertain: '结果待核对',
	deferred: '已延后',
};

export const entitlementActionLabel: Record<EntitlementAction, string> = {
	sync: '同步权益',
	claim: '领取畅听VIP',
	upgrade: '升级概念版VIP',
	reconcile_record: '核对今日记录',
};

/** 131001 / 297002 为上游「今日已领/已升级」返回码，不应误作错误。 */
export function isEntitlementAlreadyDone(detail: EntitlementEventDetail): boolean {
	return (
		detail.outcome === 'already_done' ||
		(detail.action === 'claim' && detail.upstream_code === 131001) ||
		(detail.action === 'upgrade' && detail.upstream_code === 297002)
	);
}

export function entitlementCompletionLabel(detail: EntitlementEventDetail): string {
	if (detail.upstream_code === 297002) return '今日已升级';
	if (detail.upstream_code === 131001) return '今日已领';
	return '今日已完成';
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
 * - verification_paused / upstream_unavailable -> paused_until（暂停至次日 UTC+8）
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
				return '当前处于冷却窗口，冷却结束后可继续。';
			}
			if (renewal.svip_confirmation_required) {
				return renewal.enabled
					? '尚未确认概念版VIP 有效期，点击上方「同步权益」或在播放时自动确认后生效。'
					: '尚未确认概念版VIP 有效期；自动开关已关闭，可先「同步权益」或按需手动领取。';
			}
			return renewal.enabled
				? '今日尚未领取；播放歌曲时若概念版VIP 剩余不足 48 小时或因过期被拒，将自动领取并升级。'
				: '今日尚未领取；自动开关已关闭，仍可按需手动领取。';
		case 'waiting_for_reconciliation':
			return '上次操作未能确认是否成功，需先核对今日记录后再继续，避免重复提交。';
		case 'reconciliation_exhausted':
			return '今日核对次数已用尽。系统会保留待核对状态，次日再继续。';
		case 'verification_paused':
			return '上游触发人机验证，今日自动领取已暂停；如需手动领取，可在酷狗概念版 App 手动领取后点击上方「同步权益」。';
		case 'upstream_unavailable':
			return '上游接口返回异常，今日自动领取已暂停；可查看事件日志或在酷狗概念版 App 手动领取后点击上方「同步权益」。';
		case 'rate_limited':
			return renewal.rate_limit_until && renewal.rate_limit_until > nowSec
				? '上游接口触发频率限制，请等待冷却结束。'
				: '限流冷却窗口已过，可再次尝试或同步权益查看状态。';
		case 'propagating':
			return '今日概念版VIP 已升级，上游播放约需 2 分钟生效；期间部分会员歌曲可能暂不可用。';
		case 'awaiting_upgrade':
			return '今日已领取畅听VIP，升级概念版VIP 尚未完成，可手动点击「继续升级」或在下次播放时自动续跑。';
		case 'completed':
			return renewal.svip_confirmation_required
				? '今日畅听VIP 领取与概念版VIP 升级均已完成，可点击上方「同步权益」确认上游快照。'
				: '今日畅听VIP 领取与概念版VIP 升级均已完成。';
		case 'attempts_exhausted':
			return '今日超时重试次数已用尽，可稍后在酷狗概念版 App 手动领取并点击「同步权益」，或等待次日（UTC+8）重置。';
	}
}

export interface RenewalActionControl {
	label: string;
	disabled: boolean;
}

/** 计算手动领取 VIP 按钮的准确文案与可用态。 */
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
				: { label: '核对今日记录', disabled: false };
		case 'reconciliation_exhausted':
			return { label: '今日核对次数已用尽', disabled: true };
		case 'propagating':
			return { label: '等待生效中', disabled: true };
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
					return { label: '核对今日记录', disabled: false };
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
					? '今日VIP 领取与升级已完成，可点击「同步权益」确认概念版VIP 快照。'
					: '今日VIP 领取与升级已完成。',
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
				message: '上游返回状态不明确，已进入待核对状态，请核对今日记录或查看日志。',
			};
		case 'reconciliation_exhausted':
			return { tone: 'warning', message: '今日核对次数已用尽，请查看日志或等待次日。' };
		case 'verification_paused':
			return {
				tone: 'danger',
				message: '上游要求安全或人机校验，今日自动领取 VIP 已暂停。',
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
				return { tone: 'warning', message: '本次领取未成功，请查看事件日志。' };
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
			return { tone: 'info', message: '自动领取 VIP 状态已更新。' };
	}
}

export interface CardRenewalHint {
	tone: Tone | 'muted';
	text: string;
}

/** 账号卡片上的克制一行自动领取 VIP 提示。 */
export function accountCardRenewalHint(renewal: RenewalStatus): CardRenewalHint {
	switch (renewal.state) {
		case 'propagating':
			return { tone: 'info', text: 'VIP 生效等待中' };
		case 'waiting_for_reconciliation':
			return { tone: 'warning', text: '待核对今日领取记录' };
		case 'reconciliation_exhausted':
			return { tone: 'warning', text: '今日核对次数已用尽' };
		case 'verification_paused':
			return { tone: 'danger', text: '自动领取 VIP 今日已暂停' };
		case 'upstream_unavailable':
			return { tone: 'warning', text: '自动领取 VIP 上游暂不可用' };
		case 'rate_limited':
			return { tone: 'warning', text: '自动领取 VIP 限流冷却中' };
		case 'awaiting_upgrade':
			return { tone: 'warning', text: '待升级概念版VIP' };
		case 'attempts_exhausted':
			return { tone: 'danger', text: '今日自动领取重试已用尽' };
		case 'ready':
		case 'completed':
			return renewal.enabled
				? { tone: 'muted', text: '自动领取 VIP 已开启' }
				: { tone: 'muted', text: '自动领取 VIP 未开启' };
	}
}

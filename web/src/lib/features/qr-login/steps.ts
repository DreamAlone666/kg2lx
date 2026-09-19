import type { LoginSessionStatus } from '$lib/api/types';

/**
 * 扫码登录的步骤模型。
 *
 * 把后端返回的 7 态会话状态翻译成「有序步骤 + 每步状态」，让时间轴组件保持纯展示，
 * 不必认识任何业务状态字符串。这是本次动效的主舞台：呼吸、勾选、连线生长、终态转色
 * 都派生自这里的状态，而不是由界面各自决定。
 */

export type StepState = 'done' | 'active' | 'pending' | 'failed';

export interface LoginStep {
	key: string;
	label: string;
	state: StepState;
}

/**
 * 步骤文案与 $lib/domain/labels.ts 的 sessionStatusLabel 语义相邻但**刻意不同**：
 * 前者描述「流程走到第几步」（准备 / 完成绑定），后者描述「会话当前是什么状态」
 * （准备中 / 绑定成功）。两者共存是有意的——状态胶囊回答「现在怎样」，
 * 时间轴回答「走到哪了」。修改任一处时请一并确认另一处仍然通顺。
 */
const STEP_LABELS = ['准备', '等待扫码', '等待确认', '完成绑定'] as const;

/** 各状态对应「已推进到第几步」。bound 为 4，表示四步全部完成。 */
const PROGRESS_INDEX: Record<LoginSessionStatus, number> = {
	pending: 0,
	waiting_scan: 1,
	waiting_confirm: 2,
	authorized: 3,
	bound: 4,
	// 终止态本身不代表进度，真正的断点由 stalledAt 提供；此处仅作兜底
	expired: 1,
	failed: 2
};

const isTerminal = (status: LoginSessionStatus) => status === 'expired' || status === 'failed';

/**
 * @param status    当前会话状态
 * @param stalledAt 进入终止态之前已推进到的状态。
 *                  过期与失败本身不携带进度信息，若不提供就会把断点错误地标在起点，
 *                  让用户以为「还没开始就失败了」。
 */
export function buildLoginSteps(
	status: LoginSessionStatus,
	stalledAt: LoginSessionStatus | null = null
): LoginStep[] {
	const terminated = isTerminal(status);
	const progressSource = terminated && stalledAt ? stalledAt : status;
	const activeIndex = PROGRESS_INDEX[progressSource] ?? 0;

	return STEP_LABELS.map((label, index) => {
		let state: StepState;

		if (index < activeIndex) {
			state = 'done';
		} else if (index > activeIndex) {
			state = 'pending';
		} else if (terminated) {
			// 断点所在的那一步标记为失败，而不是继续显示「进行中」
			state = 'failed';
		} else if (status === 'bound') {
			state = 'done';
		} else {
			state = 'active';
		}

		return { key: `login-step-${index}`, label, state };
	});
}

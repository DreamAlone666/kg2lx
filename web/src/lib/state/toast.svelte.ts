import type { Tone } from '$lib/domain/tone';

export interface ToastItem {
	id: number;
	tone: Tone;
	message: string;
}

export interface ToastOptions {
	message: string;
	tone?: Tone;
	/** 自动消失毫秒数；传 0 表示常驻，需用户手动关闭 */
	durationMs?: number;
}

/** 超过此数量的提示会挤占界面，故丢弃最早的一条。 */
const MAX_VISIBLE = 3;
const DEFAULT_DURATION_MS = 4000;

/**
 * 全局反馈通道。
 *
 * 复制脚本链接、刷新 Token 这类动作此前只能靠按钮文案变化表达结果：用户视线
 * 移开后再回来就无从得知成败，失败也无法被感知。这里提供统一出口，
 * 让各页面不必自造 alert（历史上已出现过用 alert 反馈复制的做法）。
 *
 * 定时器随条目一起管理并在丢弃/关闭时清理，避免悬挂定时器。
 */
class ToastStore {
	items = $state<ToastItem[]>([]);

	private seq = 0;
	private timers = new Map<number, ReturnType<typeof setTimeout>>();

	push(options: ToastOptions): number {
		const id = ++this.seq;
		const item: ToastItem = {
			id,
			tone: options.tone ?? 'neutral',
			message: options.message,
		};

		this.items = [...this.items, item];

		while (this.items.length > MAX_VISIBLE) {
			this.clearTimer(this.items[0].id);
			this.items = this.items.slice(1);
		}

		const durationMs = options.durationMs ?? DEFAULT_DURATION_MS;
		if (durationMs > 0) {
			this.timers.set(
				id,
				setTimeout(() => this.dismiss(id), durationMs)
			);
		}

		return id;
	}

	/** 便捷方法：成功提示。 */
	success(message: string, options?: Omit<ToastOptions, 'message' | 'tone'>): number {
		return this.push({ ...options, message, tone: 'success' });
	}

	/** 便捷方法：失败提示。默认常驻，确保用户不会错过错误原因。 */
	error(message: string, options?: Omit<ToastOptions, 'message' | 'tone'>): number {
		return this.push({ message, tone: 'danger', durationMs: 0, ...options });
	}

	dismiss(id: number) {
		this.clearTimer(id);
		this.items = this.items.filter((item) => item.id !== id);
	}

	private clearTimer(id: number) {
		const timer = this.timers.get(id);
		if (timer === undefined) return;
		clearTimeout(timer);
		this.timers.delete(id);
	}
}

export const toast = new ToastStore();

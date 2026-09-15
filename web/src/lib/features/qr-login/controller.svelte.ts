import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import { createVisibilityAwareTimer } from '../../utils/polling';
import type {
	QrLoginStartResponse,
	QrLoginPollResponse,
	LoginSessionStatus
} from '../../api/types';

const POLL_INTERVAL_MS = 2000;

/** 倒计时心跳。与轮询分开，因为轮询 2 秒一次会让秒数跳着走。 */
const COUNTDOWN_TICK_MS = 1000;

type BoundResult = QrLoginPollResponse & { status: 'bound' };

/**
 * 扫码登录会话控制器。
 *
 * 三件事各归其位：
 *   - 轮询节奏交给 createVisibilityAwareTimer（页面隐藏暂停、恢复可见立即补拉）
 *   - 步骤推导交给 steps.ts（本类只负责记录「推进到过哪一步」）
 *   - 倒计时由 1 秒心跳驱动，时间基准取自服务端下发的 expires_at
 */
class QrLoginController {
	sessionId = $state<string | null>(null);
	status = $state<LoginSessionStatus>('pending');
	qrBase64 = $state<string | null>(null);
	qrUrl = $state<string | null>(null);
	expiresAt = $state<number | null>(null);
	message = $state<string | null>(null);
	boundSource = $state<BoundResult | null>(null);
	/** 正在向后端申请新会话。用于给「获取二维码」按钮一个可感知的进行态。 */
	starting = $state(false);

	/**
	 * 进入终止态之前已推进到的状态。
	 * 过期与失败都不携带进度信息，必须由控制器记住，否则时间轴会把断点标在起点。
	 */
	stalledAt = $state<LoginSessionStatus | null>(null);

	/** 当前 Unix 秒（秒级），由心跳驱动，用于倒计时。 */
	private now = $state(Math.floor(Date.now() / 1000));
	/** 会话签发时刻，与 expiresAt 相减得到有效期总长，避免把 TTL 硬编码在界面里。 */
	private issuedAt = $state<number | null>(null);

	private lastProgress: LoginSessionStatus = 'pending';
	private pollInflight = false;
	private abortController: AbortController | null = null;
	private countdownTimer: ReturnType<typeof setInterval> | null = null;

	private pollTimer = createVisibilityAwareTimer({
		intervalMs: POLL_INTERVAL_MS,
		onTick: () => this.poll()
	});

	get isTerminal(): boolean {
		return this.status === 'bound' || this.status === 'expired' || this.status === 'failed';
	}

	/** 二维码剩余有效秒数；无会话时为 null。 */
	get remainingSeconds(): number | null {
		if (this.expiresAt === null) return null;
		return Math.max(0, this.expiresAt - this.now);
	}

	/** 有效期总秒数，用于把剩余时间画成进度环。 */
	get totalSeconds(): number | null {
		if (this.expiresAt === null || this.issuedAt === null) return null;
		return Math.max(1, this.expiresAt - this.issuedAt);
	}

	async start() {
		this.stop();
		this.sessionId = null;
		this.abortController = new AbortController();
		this.resetProgress();
		this.starting = true;

		try {
			const res = await adminSession.api.startQrLogin(this.abortController.signal);
			this.applyStart(res);
			this.beginPolling();
		} catch (e: any) {
			if (e.name === 'AbortError') return;
			if (handleAuthError(e)) return;
			this.status = 'failed';
			this.stalledAt = 'pending';
			this.message = e.message;
		} finally {
			this.starting = false;
		}
	}

	resume(sessionId: string) {
		if (this.sessionId === sessionId) return;
		this.stop();
		this.sessionId = sessionId;
		this.status = 'pending';
		this.beginPolling();
	}

	stop() {
		this.pollTimer.stop();
		this.stopCountdown();
		if (this.abortController) {
			this.abortController.abort();
			this.abortController = null;
		}
		this.pollInflight = false;
	}

	/** 清空全部会话状态，回到「尚未开始」的干净界面。 */
	reset() {
		this.stop();
		this.sessionId = null;
		this.qrBase64 = null;
		this.qrUrl = null;
		this.expiresAt = null;
		this.issuedAt = null;
		this.message = null;
		this.boundSource = null;
		this.stalledAt = null;
		this.status = 'pending';
		this.starting = false;
		this.resetProgress();
	}

	private resetProgress() {
		this.lastProgress = 'pending';
		this.stalledAt = null;
	}

	private applyStart(res: QrLoginStartResponse) {
		const nowSeconds = Math.floor(Date.now() / 1000);
		this.sessionId = res.session_id;
		this.status = res.status;
		this.qrBase64 = res.qr_base64;
		this.qrUrl = res.qr_url;
		this.expiresAt = res.expires_at;
		this.issuedAt = nowSeconds;
		this.now = nowSeconds;
		this.message = null;
		this.boundSource = null;
		this.stalledAt = null;
		this.lastProgress = res.status;
	}

	private beginPolling() {
		if (!this.sessionId) return;
		this.now = Math.floor(Date.now() / 1000);
		this.startCountdown();
		this.pollTimer.start(); // 会立即先拉一次，不必等满一个间隔
	}

	private startCountdown() {
		this.stopCountdown();
		this.countdownTimer = setInterval(() => {
			// 页面隐藏时不必刷新秒数；恢复可见时会重新取当前时间，不会显示错误值
			if (typeof document !== 'undefined' && document.hidden) return;
			this.now = Math.floor(Date.now() / 1000);
		}, COUNTDOWN_TICK_MS);
	}

	private stopCountdown() {
		if (this.countdownTimer !== null) {
			clearInterval(this.countdownTimer);
			this.countdownTimer = null;
		}
	}

	private async poll() {
		if (!this.sessionId || this.pollInflight) return;

		const sessionId = this.sessionId;
		this.pollInflight = true;
		if (!this.abortController) this.abortController = new AbortController();

		try {
			const res = await adminSession.api.pollQrLogin(sessionId, this.abortController.signal);
			// 会话已被切换（重置 / 重新获取）时丢弃回包
			if (this.sessionId !== sessionId) return;

			this.status = res.status;

			if (res.status === 'bound') {
				this.boundSource = res;
				this.finish();
			} else if (res.status === 'expired' || res.status === 'failed') {
				this.stalledAt = this.lastProgress;
				this.message =
					res.message ||
					(res.status === 'expired'
						? '二维码已过期，请重新获取。'
						: '登录失败，请重试。');
				this.finish();
			} else {
				this.lastProgress = res.status;
			}
		} catch (e: any) {
			if (e.name === 'AbortError') return;
			if (handleAuthError(e)) return;
			if (e.status === 404) {
				this.status = 'failed';
				this.stalledAt = this.lastProgress;
				this.message = '会话未找到，可能是服务重启过，请重新获取二维码。';
				this.finish();
			}
		} finally {
			this.pollInflight = false;
		}
	}

	private finish() {
		this.pollTimer.stop();
		this.stopCountdown();
	}
}

export const qrLoginController = new QrLoginController();

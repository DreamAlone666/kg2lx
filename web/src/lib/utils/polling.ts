/**
 * 可见性感知定时器。
 *
 * 此前两处轮询（二维码 2s、运行时日志 5s）各自裸用 setInterval，页面切到后台仍会
 * 持续发请求。这里统一为一条节奏：启动立即拉一次 → 按间隔拉 → 页面隐藏时暂停 →
 * 恢复可见时立即补拉一次（避免用户切回来先看到过期数据）→ 销毁时清理监听。
 *
 * 职责边界：本工具只管「节奏」，不介入业务。请求竞态仍由调用方自行处理
 * （现有 controller 使用单调递增 requestId + AbortController）。
 */

export interface VisibilityAwareTimerOptions {
	/** 轮询间隔（毫秒）。 */
	intervalMs: number;
	/** 每次触发时的回调，允许返回 Promise（不会被等待，异常由调用方自行捕获）。 */
	onTick: () => void | Promise<void>;
	/** 可见性判定，缺省读取 document.hidden；可注入以便测试。 */
	isVisible?: () => boolean;
}

export interface VisibilityAwareTimer {
	/** 启动并立即执行一次；重复调用等价于先 stop 再 start。 */
	start(): void;
	/** 清理定时器与监听，幂等。 */
	stop(): void;
}

export function createVisibilityAwareTimer(
	options: VisibilityAwareTimerOptions
): VisibilityAwareTimer {
	const { intervalMs, onTick } = options;
	const isVisible =
		options.isVisible ?? (() => typeof document === 'undefined' || !document.hidden);

	let intervalId: ReturnType<typeof setInterval> | null = null;
	let listening = false;
	let running = false;

	/** 定时触发：运行中且页面可见才真正拉取，隐藏时静默跳过。 */
	function tick() {
		if (!running || !isVisible()) return;
		void onTick();
	}

	/** 恢复可见时立即补拉，让用户不必等待下一个间隔。 */
	function handleVisibilityChange() {
		if (!running || !isVisible()) return;
		void onTick();
	}

	function stop() {
		running = false;
		if (intervalId !== null) {
			clearInterval(intervalId);
			intervalId = null;
		}
		if (listening) {
			document.removeEventListener('visibilitychange', handleVisibilityChange);
			listening = false;
		}
	}

	function start() {
		stop();
		running = true;
		void onTick();
		intervalId = setInterval(tick, intervalMs);
		if (!listening && typeof document !== 'undefined') {
			document.addEventListener('visibilitychange', handleVisibilityChange);
			listening = true;
		}
	}

	return { start, stop };
}

/**
 * 可见性感知定时器。
 *
 * 此前两处轮询（二维码 2s、运行时日志 5s）各自裸用 setInterval，页面切到后台仍会
 * 持续发请求。这里统一为一条节奏：启动立即拉一次 → 按间隔拉 → 页面隐藏时暂停 →
 * 恢复可见时立即补拉一次（避免用户切回来先看到过期数据）→ 销毁时清理监听。
 *
 * 另外提供 pause / resume：有些页面（运行时日志）是长时间停留在屏幕上的，
 * 用户需要一个「停下来看清一行」的手段，而不能只能靠切走标签页。
 * 注：refreshNow / isPaused 曾在上一轮作为「无调用方的投机性 API」被移除，
 * 这里的 pause / resume 是**有真实调用方**的重新引入，不是同一种东西。
 *
 * 职责边界：本工具只管「节奏」，不介入业务。请求竞态仍由调用方自行处理
 * （现有 controller 用单调递增 requestId 丢弃过期回包，并对轮询做在途去重）。
 * 注意本工具**不会**取消在途请求——它不持有 AbortSignal，也不该持有。
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
	/**
	 * 用户主动暂停：只停间隔，保留可见性监听。
	 *
	 * 与「页面隐藏」是两种不同的停止，不能合并成一种：隐藏是自动的、静默的，
	 * 恢复可见即续跑；暂停是用户显式要求的，必须一直保持到用户自己恢复——
	 * 否则用户切个标签页再回来就发现暂停被悄悄取消了。
	 */
	pause(): void;
	/** 从暂停恢复：立即补拉一次，避免用户回来先看到过期数据。 */
	resume(): void;
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
	let paused = false;

	function clearIntervalIfAny() {
		if (intervalId !== null) {
			clearInterval(intervalId);
			intervalId = null;
		}
	}

	/** 定时触发：运行中、未被暂停、且页面可见才真正拉取。 */
	function tick() {
		if (!running || paused || !isVisible()) return;
		void onTick();
	}

	/** 恢复可见时立即补拉，让用户不必等待下一个间隔。 */
	function handleVisibilityChange() {
		if (!running || paused || !isVisible()) return;
		void onTick();
	}

	function stop() {
		running = false;
		paused = false;
		clearIntervalIfAny();
		if (listening) {
			document.removeEventListener('visibilitychange', handleVisibilityChange);
			listening = false;
		}
	}

	function start() {
		stop();
		running = true;
		// 首次拉取刻意**不**做可见性判断：它是「把首屏填上」的一次性预取，而不是
		// 轮询节奏的一部分。页面若在后台标签里挂载，这一次请求仍然值得发出——等用户
		// 切过来时数据已经在手里，而不是先看到空态再等一个来回。
		// 这与 tick() / handleVisibilityChange() 的语义不同，那两处必须判可见性。
		void onTick();
		intervalId = setInterval(tick, intervalMs);
		if (!listening && typeof document !== 'undefined') {
			document.addEventListener('visibilitychange', handleVisibilityChange);
			listening = true;
		}
	}

	function pause() {
		if (!running) return;
		paused = true;
		clearIntervalIfAny();
	}

	function resume() {
		if (!running || !paused) return;
		paused = false;
		void onTick();
		intervalId = setInterval(tick, intervalMs);
	}

	return { start, stop, pause, resume };
}

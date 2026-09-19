/**
 * 展示格式化 —— 把后端返回的原始数值统一成界面可读文本。
 *
 * 此前每个页面各写一份 `new Date(ts * 1000).toLocaleString()`，同一份数据在不同
 * 页面可能呈现不同格式；集中到此处后格式只定义一次。
 * 纯函数、无副作用，可安全用于派生值。
 */

/**
 * Unix 秒级时间戳 → 定长本地时间 `YYYY-MM-DD HH:mm:ss`（24 小时制）。
 *
 * 刻意不用 `toLocaleString()`：它的产物随浏览器 locale 变化（en-US 实测为
 * `9/15/2026, 9:27:36 PM`），既与日志列的 24 小时制同屏冲突，又会在窄信息格里
 * 被折成两行。手工补齐后各段宽度恒定，配合 `.tabular` 可稳定纵向对齐。
 *
 * 时间部分直接复用 formatClockTime，使「日期时间」与「纯时间」在制式上不可能分叉。
 */
export function formatDateTime(ts: number | null | undefined, fallback = '从未'): string {
	if (ts === null || ts === undefined || !Number.isFinite(ts)) return fallback;
	const d = new Date(ts * 1000);
	const date = [d.getFullYear(), pad2(d.getMonth() + 1), pad2(d.getDate())].join('-');
	return `${date} ${formatClockTime(ts)}`;
}

function pad2(n: number): string {
	return n.toString().padStart(2, '0');
}

/** Unix 秒级时间戳 → 本地时间（24 小时制），用于日志这类需要精确到秒的场景。 */
export function formatClockTime(ts: number | null | undefined): string {
	if (ts === null || ts === undefined || !Number.isFinite(ts)) return '--:--:--';
	const d = new Date(ts * 1000);
	// 同样手工补齐而非 toLocaleTimeString：后者在部分 locale 下会给出 12 小时制或
	// 前后缀（如「下午 9:14:40」），宽度不定，会破坏表格列的纵向对齐。
	return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

/**
 * 日志行的时刻：当天只给 `HH:mm:ss`，跨天时补 `MM-DD`。
 *
 * 运行时日志是**跨天混排**的：线上 20 行窗口实测横跨 2026-05 ~ 2026-09。
 * 只显示时分秒会让「21:14 → 14:50」读起来像同一天的几小时，实际相隔四个月，
 * 时间列于是从「排序依据」退化成了错误信息——用户会把四个月前的失败当成刚发生的。
 *
 * 当天不补日期是为了保住 8 字符的紧凑宽度（绝大多数行都是当天），跨天才加宽到
 * 14 字符。精确到年仍可在 title 里读到（见 LogsPanel），不必为长尾牺牲常态列宽。
 */
export function formatLogTime(ts: number | null | undefined, now: number = Date.now()): string {
	const clock = formatClockTime(ts);
	if (ts === null || ts === undefined || !Number.isFinite(ts)) return clock;
	const d = new Date(ts * 1000);
	const t = new Date(now);
	const sameDay =
		d.getFullYear() === t.getFullYear() &&
		d.getMonth() === t.getMonth() &&
		d.getDate() === t.getDate();
	return sameDay ? clock : `${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${clock}`;
}

/**
 * 延迟毫秒 → 紧凑文本。超过 1 秒改用秒为单位，
 * 避免 4 位以上的数字挤占列宽、破坏表格的纵向扫读。
 */
export function formatLatency(ms: number | null | undefined): string {
	if (ms === null || ms === undefined || !Number.isFinite(ms) || ms < 0) return '—';
	if (ms < 1000) return `${Math.round(ms)}ms`;
	return `${(ms / 1000).toFixed(1)}s`;
}

/** 截断长标识符，保留可辨识前缀（排查时用于快速比对是否同一次请求）。 */
export function shortId(id: string | null | undefined, length = 7): string {
	if (!id) return '—';
	return id.length <= length ? id : id.slice(0, length);
}

/** 剩余秒数 → `分:秒`，用于二维码有效期倒计时。 */
export function formatCountdown(seconds: number): string {
	const total = Math.max(0, Math.ceil(seconds));
	const minutes = Math.floor(total / 60);
	const rest = total % 60;
	return `${minutes}:${rest.toString().padStart(2, '0')}`;
}

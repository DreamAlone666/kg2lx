/**
 * 展示格式化 —— 把后端返回的原始数值统一成界面可读文本。
 *
 * 此前每个页面各写一份 `new Date(ts * 1000).toLocaleString()`，同一份数据在不同
 * 页面可能呈现不同格式；集中到此处后格式只定义一次。
 * 纯函数、无副作用，可安全用于派生值。
 */

/** Unix 秒级时间戳 → 本地日期时间。 */
export function formatDateTime(ts: number | null | undefined, fallback = '从未'): string {
	if (ts === null || ts === undefined || !Number.isFinite(ts)) return fallback;
	return new Date(ts * 1000).toLocaleString();
}

/** Unix 秒级时间戳 → 本地时间（24 小时制），用于日志这类需要精确到秒的场景。 */
export function formatClockTime(ts: number | null | undefined): string {
	if (ts === null || ts === undefined || !Number.isFinite(ts)) return '--:--:--';
	return new Date(ts * 1000).toLocaleTimeString([], {
		hour: '2-digit',
		minute: '2-digit',
		second: '2-digit',
		hour12: false,
	});
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

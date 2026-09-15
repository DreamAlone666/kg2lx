import type { Tone } from '$lib/domain/tone';

/**
 * 语义色调 → 类名映射。
 *
 * 必须是**静态字面量**：Tailwind 只能扫描源码中的完整类名，
 * 任何 `bg-${tone}-soft` 式的运行时拼接都不会生成样式。
 *
 * 单独成文件而非放在 StatusBadge 的 module 上下文里，是为了让状态胶囊与
 * 全局提示（Toaster）共用同一套色值，避免两处各写一份而逐渐走样。
 */

/** 柔和底胶囊：用于状态标记。 */
export const toneChip: Record<Tone, string> = {
	success: 'bg-success-soft text-success',
	warning: 'bg-warning-soft text-warning',
	danger: 'bg-danger-soft text-danger',
	info: 'bg-info-soft text-info',
	neutral: 'bg-neutral-soft text-neutral',
};

/** 实心点：用于在胶囊内强化状态存在感，或在时间轴上表示节点。 */
export const toneDot: Record<Tone, string> = {
	success: 'bg-success',
	warning: 'bg-warning',
	danger: 'bg-danger',
	info: 'bg-info',
	neutral: 'bg-neutral/40',
};

/** 提示条表面：柔和底 + 同色描边 + 同色文字。 */
export const toneSurface: Record<Tone, string> = {
	success: 'border-success/25 bg-success-soft text-success',
	warning: 'border-warning/25 bg-warning-soft text-warning',
	danger: 'border-danger/25 bg-danger-soft text-danger',
	info: 'border-info/25 bg-info-soft text-info',
	neutral: 'border-border bg-card text-foreground',
};

export function toneChipClass(tone: Tone): string {
	return toneChip[tone] ?? toneChip.neutral;
}

export function toneDotClass(tone: Tone): string {
	return toneDot[tone] ?? toneDot.neutral;
}

export function toneSurfaceClass(tone: Tone): string {
	return toneSurface[tone] ?? toneSurface.neutral;
}

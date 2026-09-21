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

/**
 * 实心胶囊：与 toneChip 同一色系、高出一档的强度。
 *
 * 用于「同一色相里谁更重要」——目前唯一的调用方是最高一档会员权益（见
 * domain/tone.ts 的 vipEntitlementEmphasis）。底一律取该 tone 的**实心令牌**
 * （--color-success / --color-warning / ...），文字取 --color-background；也就是
 * 不新增任何色值，只把既有的「实心 / 柔和底」两级关系用满（layout.css 里每个 tone
 * 本来就各有两个令牌）。
 *
 * 文字用 `text-background` 而不是 `text-white`：两者此刻取值相同（都是 #FFFFFF），
 * 但语义不同——它表达的是「这块实心面上的前景色」，将来抬升面换色时能跟着走。
 *
 * ---------- 对比度（自算，不是抄来的） ----------
 * 算法是 WCAG 的 sRGB 相对亮度：每个通道 c/255 后线性化（≤0.03928 走 c/12.92，
 * 否则 ((c+0.055)/1.055)^2.4），L = 0.2126R + 0.7152G + 0.0722B，对比度 =
 * (L亮+0.05)/(L暗+0.05)。底色取本仓 layout.css @theme 里的令牌实测值。胶囊文字是
 * 12px 常规字重（text-xs font-medium），够不上 WCAG 的「大字」宽松档
 * （≥18.66px 粗体 / ≥24px），所以逐档都得 ≥4.5:1。
 *
 * 承载面：本文件诞生时胶囊所在的面都是卡片（#FFFFFF）。若将来把它放到页面底
 * （#FAFAFA，本仓最暗的浅底）上，各档对比度会低 0.25~0.32——最差的 info 由 5.93 降到
 * 5.69，仍在 4.5:1 之上，故这一档不必因为换面而改值。
 */
export const toneChipSolid: Record<Tone, string> = {
	/** #166534 对 #FFFFFF：7.13:1。 */
	success: 'bg-success text-background',
	/** #92400E 对 #FFFFFF：7.09:1（L 0.0981 / 1.0000）。会员权益那一档走的就是它。 */
	warning: 'bg-warning text-background',
	/** #B91C1C 对 #FFFFFF：6.47:1。 */
	danger: 'bg-danger text-background',
	/** #0369A1 对 #FFFFFF：5.93:1 —— 五档最低，余量不大；这条蓝色令牌不能再调浅。 */
	info: 'bg-info text-background',
	/** #52525B 对 #FFFFFF：7.73:1。中性档实心后偏深灰，与状态色仍靠色相区分。 */
	neutral: 'bg-neutral text-background',
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

export function toneChipSolidClass(tone: Tone): string {
	return toneChipSolid[tone] ?? toneChipSolid.neutral;
}

export function toneSurfaceClass(tone: Tone): string {
	return toneSurface[tone] ?? toneSurface.neutral;
}

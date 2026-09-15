/**
 * 动效常量 —— 与 routes/layout.css 的 @theme 一一对应，两侧改动必须同步。
 *
 * 为什么不直接从 CSS 读：CSS 只覆盖样式层面的过渡；Svelte transition 与
 * Web Animations API 需要具体数值，而 getComputedStyle 在首帧与 SSR 场景不可靠。
 * 保持为纯常量便于 tree-shake 与类型推导。
 */

/** 微交互：按钮按压、图标形变。 */
export const DURATION_FAST = 150;

/** 状态过渡：卡片进入、日志行高亮衰减。 */
export const DURATION_BASE = 200;

/** 页面与视图切换。 */
export const DURATION_SLOW = 300;

/** 进入用缓出。 */
export const EASE_OUT_SOFT = 'cubic-bezier(0.22, 1, 0.36, 1)';

/** 退出用缓入。 */
export const EASE_IN_SOFT = 'cubic-bezier(0.4, 0, 1, 1)';

/** 回弹用轻微过冲。 */
export const EASE_SPRING = 'cubic-bezier(0.34, 1.56, 0.64, 1)';

/**
 * 只读判定当前是否应降级动效。
 * CSS 侧已在 layout.css 全局降级，但 JS 驱动的过渡不受其影响，故命令式代码
 * （View Transitions、Svelte transition 参数）必须先查询这里。
 * 需要随系统设置变化实时更新的场景请改用 useReducedMotion。
 */
export function prefersReducedMotion(): boolean {
	if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
	return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

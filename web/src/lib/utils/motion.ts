/**
 * 动效常量 —— 与 routes/layout.css 的 @theme 一一对应，两侧改动必须同步。
 *
 * 为什么不直接从 CSS 读：CSS 只覆盖样式层面的过渡；Svelte transition 与
 * Web Animations API 需要具体数值，而 getComputedStyle 在首帧与 SSR 场景不可靠。
 * 保持为纯常量便于 tree-shake 与类型推导。
 *
 * 本模块刻意保持**完整**的档位集合（3 个时长 + 3 条缓动），与 CSS 侧对齐：
 * 只保留当前已被引用的那几项会让镜像关系失真，后续新增动效时又得回来补，
 * 反而更容易与 CSS 走偏。
 *
 * [TODO] 一次性动画目前有两种互不相同的落地方式，都是为了绕开 Svelte 的作用域样式裁剪：
 *   - CopyField：组件内 <style> + 关键帧，类名静态出现，会被保留
 *   - LogsPanel：layout.css 的全局关键帧 + 内联 animation，因类名只出现在 JS 字符串里，
 *     作用域样式有可能判定其未使用而裁掉
 * 两种都能跑，但同一需求两种写法会让后来者无从选择，也容易再踩一次同样的坑。
 * 方向：在动效层统一约定一种做法（倾向全局关键帧 + 内联 animation，副作用更小），
 * 并把原因写在这里。
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

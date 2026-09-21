/**
 * 原语层新增文件一律用「文件夹 + index.ts」这一上游约定（shadcn-svelte CLI 的产物
 * 就是这个形状），因此将来的 `shadcn-svelte add` 可以直接把文件落进来而不冲突。
 *
 * 刻意**不**进 `$lib/components/ui/index.ts` 那个扁平文件桶：桶里全是
 * `export { default as X } from './x.svelte'` 的单组件形态，而这里按命名空间使用
 * （Switch 是一个复合控件，Root/Thumb 是它的内部结构），混进去会让两种约定在同一个
 * 文件里打架。既有 9 个扁平文件的迁移不在本轮范围。
 */
export { default as Switch } from './switch.svelte';

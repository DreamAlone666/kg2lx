import { Tooltip as TooltipPrimitive } from 'bits-ui';
import TooltipContent from './tooltip.svelte';

/**
 * Tooltip 命名空间：形状与 bits-ui 的 `Tooltip` 对齐（Root / Trigger / Content /
 * Provider），因此调用点写 `import * as Tooltip from '$lib/components/ui/tooltip'`
 * 之后，读起来与上游文档一致——换原语的人不必再学第二套命名。
 *
 * 采用「文件夹 + index.ts」而不是本仓既有 ui/ 的扁平文件：这是 shadcn-svelte CLI
 * 的产物形状，将来 `shadcn-svelte add` 能把文件直接落进来。同理**不**往
 * `$lib/components/ui/index.ts` 那个扁平桶里塞：桶里全是单组件形态
 * （`export { default as X }`），这里是按命名空间用的多组件模块，混在一起会让
 * 两种约定在同一个文件里打架。既有 9 个扁平文件的迁移不在本轮。
 *
 * 只有 Content 换成本仓实现（浅色浮起面 + 内置 Portal，见 ./tooltip.svelte）；
 * Root / Trigger 原样透传——两者都没有可定制的表面，包一层只会多出同名的空壳。
 * Provider 也直接透传，它不渲染任何 DOM，只是个上下文。
 *
 * Trigger 有两个默认值需要在用时留意（上游行为，不是本仓改的）：
 * - 它默认渲染成 `<button>`。触发元素不是按钮时（本仓三处分别是 span / span / h1）
 *   必须走 `child` 渲染委托，否则会凭空多出一个按钮：多一个 tab 停，且 h1 里嵌按钮
 *   属于语义错误。
 * - 它默认带 `type="button"`，这个属性会随 `{...props}` 落到委托的元素上。非按钮元素
 *   应当传 `type={null}` 抹掉（bits-ui 内部 mergeProps 保留 null，Svelte 摊开属性时
 *   会把 null 当作「移除该属性」）。
 */
export const Root = TooltipPrimitive.Root;
export const Trigger = TooltipPrimitive.Trigger;
export const Content = TooltipContent;
export const Provider = TooltipPrimitive.Provider;

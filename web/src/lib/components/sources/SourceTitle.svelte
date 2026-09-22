<script lang="ts">
	import { tick } from 'svelte';
	import { Button, Input } from '$lib/components/ui';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { sourcesController } from '$lib/features/sources/controller.svelte';
	import { labelOf, providerLabel } from '$lib/domain';
	import { toast } from '$lib/state/toast.svelte';
	import { cn } from '$lib/utils.js';
	import type { SourceDetail } from '$lib/api/types';

	/**
	 * 详情页页头：标题 + 就地改名。
	 *
	 * 改名做成「原地替换 <h1>」而不是弹窗或另起一段表单：这一页被改动的只有标题，
	 * 把编辑框挪到别处会让用户失去「改的就是这一行」的确认感，也让键盘焦点离开标题区。
	 *
	 * 标题与账号 ID 的分工：标题由用户自定、随时可改，账号 ID 才是稳定身份，
	 * 因此账号 ID 常驻显示在标题下方——否则改名之后这一页就没有可用来对账的锚点了。
	 */
	let { source, onRenamed }: { source: SourceDetail; onRenamed?: () => void } = $props();

	let editing = $state(false);
	let draft = $state('');
	let inputEl = $state<HTMLInputElement | null>(null);
	let renameButton = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);

	/** 与提示行配对的无障碍 id；用 $derived 而非普通常量，避免把 props 读成一次性快照。 */
	const hintId = $derived(`source-title-hint-${source.source_id}`);

	/**
	 * 按**码点**计数，不用 `draft.length`：后者数的是 UTF-16 码元，
	 * 一个 emoji 会算成 2，用户每放一个 emoji 就凭空多消耗一格建议额度。
	 */
	const count = $derived([...draft].length);
	const overAdvisory = $derived(count > source.name_advisory_chars);
	const isEmptyDraft = $derived(draft.trim() === '');
	/**
	 * 「重命名」按钮的禁用条件，只需要看刷新。
	 *
	 * 刻意不看 `savingName`：这个按钮只在展示态存在，而保存中我们正处于编辑态，
	 * 重复提交已由「保存」自身的 pending 挡住。把它算进来的唯一效果是——若某次保存
	 * 的请求始终不落地，用户取消退出后会连「重命名」都点不动，只能刷新页面。
	 */
	const busy = $derived(Boolean(sourcesController.refreshing[source.source_id]));

	async function startEdit() {
		draft = source.name;
		editing = true;
		// 点击「重命名」会在同一帧里卸载这个按钮；不显式接管焦点的话它会掉回 <body>，
		// 键盘用户接下来要重新 Tab 过整页才能回到输入框。
		await tick();
		inputEl?.focus();
		inputEl?.select();
	}

	async function exitEdit() {
		editing = false;
		await tick();
		renameButton?.focus();
	}

	function cancel() {
		return exitEdit();
	}

/**
 * 保存期间仍允许取消。
 *
 * 看起来更「正确」的做法是保存期间禁用取消（请求已发出、撤回不了），但实测过它的代价：
 * 请求一旦不落地（连接被丢弃、网络分区），`savingName` 会一直停在进行中，于是取消被禁、
 * Escape 被拦，用户被困在这张表单里，只能靠离开页面脱身。宁可让「取消」在慢请求下不能
 * 真正撤回（写已经发生，完成后的提示会把实情说出来），也不要新造一个无路可退的状态——
 * 本仓库已经两次栽在卡死状态上。
 */
function handleKeydown(e: KeyboardEvent) {
	if (e.key === 'Escape') {
		e.preventDefault();
		cancel();
	}
}

	async function save() {
		const submitted = draft.trim();

		const result = await sourcesController.renameSource(
			source.source_id,
			submitted === '' ? null : submitted
		);

		// 失败刻意不弹提示：页面已经把 sourcesController.error 渲染成横幅，
		// 再弹一条等于把同一句话讲两遍。保持编辑态、保留用户输入，让他直接改。
		if (result === 'failed') return;

		const saved = sourcesController.currentSource?.name ?? submitted;
		if (submitted === '') {
			toast.success(`已恢复默认标题「${saved}」。`);
		} else if (submitted !== saved) {
			// 只说「服务端规范化过」，不点名具体字符：空白折叠、控制字符过滤与 `*/`
			// 移除都落在这一条分支上，点名其中一个就是在断言一件没验证过的事。
			toast.push({
				tone: 'warning',
				message: `已保存为「${saved}」。脚本头部只能容纳单行纯文本，标题中的空白与特殊字符已由服务端规范化。`
			});
		} else {
			toast.success('标题已保存。');
		}

		onRenamed?.();
		await exitEdit();
	}
</script>

<div class="min-w-0">
	{#if editing}
		<form
			class="space-y-2"
			onsubmit={(e) => {
				e.preventDefault();
				save();
			}}
		>
			<!--
				刻意不设 maxlength：`name_advisory_chars` 只是官方建议，后端接受超长标题
				且渲染路径只净化不截断。用 maxlength 会把它变成事实上的硬限制，
				让用户以为超长标题存不进去——本仓库已有过「界面文案比字段语义更强」的记录。
			-->
			<Input
				bind:value={draft}
				bind:ref={inputEl}
				aria-label="音源标题"
				aria-describedby={hintId}
				onkeydown={handleKeydown}
			/>

			<!--
				计数行必须始终说真话：只报「已输入几个」与「建议几个」，
				绝不写成 n/24 这种分数形式——那会被读成还剩多少额度，而它并不封顶。
			-->
			<div id={hintId} class="flex flex-wrap gap-x-2 text-xs">
				<span class={cn('tabular', overAdvisory ? 'text-warning' : 'text-muted-foreground')}>
					{#if overAdvisory}
						已输入 {count} 个字符（超出官方建议的 {source.name_advisory_chars} 个，不影响导入）
					{:else}
						已输入 {count} 个字符（官方建议不超过 {source.name_advisory_chars} 个）
					{/if}
				</span>
				{#if isEmptyDraft}
					<span class="text-muted-foreground">
						留空保存将恢复默认标题「{source.default_name}」
					</span>
				{/if}
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<Button
					type="submit"
					size="sm"
					disabled={draft.trim() === source.name}
					pending={sourcesController.savingName}
					pendingLabel="保存中…"
				>
					保存
				</Button>
				<!--
					必须显式写 type="button"：都在 <form> 内，浏览器默认 type="submit"，
					漏写会把「取消」变成保存。
				-->
				<Button type="button" variant="ghost" size="sm" onclick={cancel}>取消</Button>
			</div>
		</form>
	{:else}
		<div class="flex items-center gap-2">
			<!--
				截断的标题：完整值改用 Tooltip 提示（原生 title 只能等长按才出现、
				样式与延迟也不可控）。标题可能远长于容器（后端只净化不截断），
				这一处是页面上最需要「看到全名」的地方。

				触发元素是 h1，所以走 `child` 渲染委托——Trigger 默认渲染成 <button>，
				直接包会让标题变成按钮（语义错误），也会多出一个按钮样式的可聚焦元素。
				委托后 h1 由 Trigger 授予 tabindex=0（默认值），键盘用户也能读到全名。
				`type={null}` 抹掉 Trigger 默认带的 `type="button"`（h1 不接受该属性）。
				title 已删除，否则浏览器提示会与本浮层同时出现。
			-->
			<Tooltip.Root>
				<Tooltip.Trigger type={null}>
					{#snippet child({ props })}
						<h1
							class="min-w-0 truncate text-2xl font-bold tracking-tight"
							{...props}
						>
							{source.name}
						</h1>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>{source.name}</Tooltip.Content>
			</Tooltip.Root>
			<!--
				只有文字、没有图标，也不用 size="icon"：本项目的图标目前只表达
				「状态反馈」（复制成功/失败），铅笔会成为第一个「动作图标」，
				与既有约定不符；动作身份交给文字承担。
			-->
			<Button
				variant="ghost"
				size="sm"
				class="shrink-0"
				bind:ref={renameButton}
				disabled={busy}
				onclick={startEdit}
			>
				重命名
			</Button>
		</div>
	{/if}

	<div class="mt-1 flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
		<span>账号 {source.account.userid} · 平台 {labelOf(providerLabel, source.provider)}</span>
	</div>
</div>

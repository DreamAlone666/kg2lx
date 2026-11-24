<script lang="ts">
	import { tick } from 'svelte';
	import { Button, Input } from '$lib/components/ui';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { accountsController } from '$lib/features/accounts/controller.svelte';
	import { toast } from '$lib/state/toast.svelte';
	import { cn } from '$lib/utils.js';
	import type { AccountSourceItem } from '$lib/api/types';

	/**
	 * 音源原地重命名控件。
	 *
	 * - 支持作为页头主标题（as="h1"）或多源卡片小标题（as="h2"）
	 * - 使用独立的 accountsController.savingSourceName[source.source_id]，
	 *   绝不被账号级动作（刷新 Token / 同步权益 / 手动领取 VIP）锁死
	 */
	let {
		source,
		as = 'h1',
		onRenamed
	}: {
		source: AccountSourceItem;
		as?: 'h1' | 'h2';
		onRenamed?: (sourceId: string) => void;
	} = $props();

	let editing = $state(false);
	let draft = $state('');
	let inputEl = $state<HTMLInputElement | null>(null);
	let renameButton = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);

	const uid = $props.id();
	const hintId = `source-name-hint-${uid}`;
	const count = $derived([...draft].length);
	const overAdvisory = $derived(count > source.name_advisory_chars);
	const overLimit = $derived(new TextEncoder().encode(draft.trim()).byteLength > 256);
	const isEmptyDraft = $derived(draft.trim() === '');
	const saving = $derived(Boolean(accountsController.savingSourceName[source.source_id]));

	async function startEdit() {
		draft = source.name;
		editing = true;
		await tick();
		inputEl?.focus();
		inputEl?.select();
	}

	async function exitEdit() {
		editing = false;
		await tick();
		renameButton?.focus();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.preventDefault();
			void exitEdit();
		}
	}

	async function save() {
		const submitted = draft.trim();
		const previousName = source.name;
		if ((submitted || source.default_name) === previousName) {
			await exitEdit();
			return;
		}
		const res = await accountsController.renameSource(
			source.source_id,
			submitted === '' ? null : submitted
		);

		if (res.status === 'failed') {
			toast.error(res.message);
			return;
		}

		if (res.status !== 'ok') return;
		const { name } = res.value;
		if (submitted === '') {
			toast.success(`已恢复默认标题「${name}」。`);
		} else if (submitted !== name) {
			toast.push({
				tone: 'warning',
				message: `已保存为「${name}」。脚本头部只能容纳单行纯文本，标题中的空白与特殊字符已由服务端规范化。`
			});
		} else {
			toast.success('标题已保存。');
		}

		if (name !== previousName) onRenamed?.(source.source_id);
		await exitEdit();
	}
</script>

<div class="min-w-0">
	{#if editing}
		<form
			class="space-y-2"
			onsubmit={(e) => {
				e.preventDefault();
				void save();
			}}
		>
			<Input
				bind:value={draft}
				bind:ref={inputEl}
				aria-label="音源标题"
				aria-describedby={hintId}
				onkeydown={handleKeydown}
			/>

			<div id={hintId} class="flex flex-wrap gap-x-2 text-xs">
				<span class={cn('tabular', (overLimit || overAdvisory) ? 'text-warning' : 'text-muted-foreground')}>
					{#if overLimit}
						当前输入超过 256 字节，净化后仍超出会被拒绝。
					{:else if overAdvisory}
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
					disabled={(draft.trim() || source.default_name) === source.name}
					pending={saving}
					pendingLabel="保存中…"
				>
					保存
				</Button>
				<Button type="button" variant="ghost" size="sm" onclick={() => exitEdit()}>取消</Button>
			</div>
		</form>
	{:else}
		<div class="flex items-center gap-2">
			<Tooltip.Root>
				<Tooltip.Trigger type={null}>
					{#snippet child({ props })}
						{#if as === 'h1'}
							<h1
								class="min-w-0 truncate text-2xl font-bold tracking-tight"
								{...props}
							>
								{source.name}
							</h1>
						{:else}
							<h2
								class="min-w-0 truncate text-base font-semibold tracking-tight"
								{...props}
							>
								{source.name}
							</h2>
						{/if}
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>{source.name}</Tooltip.Content>
			</Tooltip.Root>

			<Button
				variant="ghost"
				size="sm"
				class="shrink-0"
				bind:ref={renameButton}
				onclick={startEdit}
			>
				重命名
			</Button>
		</div>
	{/if}
</div>

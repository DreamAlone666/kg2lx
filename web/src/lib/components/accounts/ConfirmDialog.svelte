<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Button } from '$lib/components/ui';
	import * as Dialog from '$lib/components/ui/dialog';

	type Result =
		| { status: 'ok' | 'redirected' | 'busy' }
		| { status: 'failed'; message: string };

	let {
		triggerLabel,
		triggerClass,
		title,
		confirmLabel,
		pendingLabel,
		disabled,
		pending,
		description,
		impacts,
		onConfirm
	}: {
		triggerLabel: string;
		triggerClass: string;
		title: string;
		confirmLabel: string;
		pendingLabel: string;
		disabled: boolean;
		pending: boolean;
		description: Snippet;
		impacts: Snippet;
		onConfirm: () => Promise<Result>;
	} = $props();

	let open = $state(false);
	let error = $state<string | null>(null);
	let cancelRef = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);

	async function confirm() {
		error = null;
		const result = await onConfirm();
		if (result.status === 'failed') {
			error = result.message;
		} else if (result.status === 'ok' || result.status === 'redirected') {
			open = false;
		}
	}
</script>

<Dialog.Root bind:open onOpenChange={(next) => { if (next) error = null; }}>
	<Dialog.Trigger {disabled}>
		{#snippet child({ props })}
			<Button {...props} variant="ghost" size="sm" class={triggerClass}>
				{triggerLabel}
			</Button>
		{/snippet}
	</Dialog.Trigger>

	<Dialog.Content
		variant="center"
		class="sm:max-w-md"
		escapeKeydownBehavior={pending ? 'ignore' : 'close'}
		interactOutsideBehavior={pending ? 'ignore' : 'close'}
		onOpenAutoFocus={(event) => {
			if (!cancelRef) return;
			event.preventDefault();
			cancelRef.focus();
		}}
	>
		<div class="shrink-0 px-6 pt-6 pb-2">
			<Dialog.Title class="text-base font-semibold tracking-tight text-foreground">
				{title}
			</Dialog.Title>
		</div>

		<div class="min-h-0 space-y-3 overflow-y-auto px-6 py-2 text-sm leading-relaxed text-muted-foreground">
			<Dialog.Description class="wrap-anywhere">{@render description()}</Dialog.Description>
			<ul class="space-y-1 rounded-lg bg-muted/50 px-3.5 py-2.5 text-xs leading-relaxed text-muted-foreground">
				{@render impacts()}
			</ul>
			{#if error}
				<p class="text-xs leading-relaxed text-danger" role="alert">{error}</p>
			{/if}
		</div>

		<div class="flex shrink-0 flex-col-reverse gap-2 border-t border-border/70 bg-muted/30 px-6 py-4 sm:flex-row sm:justify-end">
			<Button variant="outline" size="sm" disabled={pending} bind:ref={cancelRef} onclick={() => (open = false)}>
				取消
			</Button>
			<Button variant="destructive" size="sm" {pending} {pendingLabel} onclick={confirm}>
				{confirmLabel}
			</Button>
		</div>
	</Dialog.Content>
</Dialog.Root>

<script lang="ts">
	import { Alert, Button, Input, Card, CardContent } from '$lib/components/ui';
	import { connectController } from '$lib/features/connect/controller.svelte';

	/**
	 * 连接表单。字段状态与提交逻辑都在 features/connect 控制器里，
	 * 本组件只负责表达：标签、说明、错误与提交态。
	 */
	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		await connectController.connect();
	}
</script>

<Card>
	<CardContent class="p-6">
		<form onsubmit={handleSubmit} class="space-y-5">
			<div class="space-y-2">
				<label for="baseUrl" class="block text-sm font-medium">服务器地址</label>
				<Input
					id="baseUrl"
					bind:value={connectController.baseUrl}
					placeholder="留空使用当前域名"
					autocomplete="url"
					spellcheck="false"
				/>
				<p class="text-xs leading-relaxed text-muted-foreground">
					留空将使用当前域名（Same Origin）。开发环境可填
					<code class="rounded bg-muted px-1 py-0.5 font-mono text-caption">/backend</code>
					快速连接本地服务。
				</p>
			</div>

			<div class="space-y-2">
				<label for="token" class="block text-sm font-medium">管理员 Token</label>
				<Input
					id="token"
					type="password"
					bind:value={connectController.token}
					placeholder="ADMIN_TOKEN"
					autocomplete="current-password"
					required
				/>
			</div>

			{#if connectController.error}
				<Alert>{connectController.error}</Alert>
			{/if}

			<Button
				type="submit"
				size="lg"
				class="w-full"
				pending={connectController.loading}
				pendingLabel="连接中…"
			>
				连接
			</Button>
		</form>
	</CardContent>
</Card>

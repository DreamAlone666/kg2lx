import { adminSession, handleAuthError } from '$lib/state/admin-session.svelte';
import type { AccountItem, RenewalStatus } from '$lib/api/types';
import { describeApiError } from '$lib/domain';

export type AccountAction = 'refresh' | 'sync' | 'claim';

type SaveResult<T> =
	| { status: 'ok'; value: T }
	| { status: 'failed'; message: string }
	| { status: 'redirected' | 'busy' };

class AccountsController {
	accounts = $state<AccountItem[]>([]);
	currentAccount = $state<AccountItem | null>(null);
	loadingList = $state(false);
	loadingDetail = $state(false);
	error = $state<string | null>(null);
	accountActions = $state<Partial<Record<string, AccountAction>>>({});
	savingAutoClaim = $state<Partial<Record<string, boolean>>>({});
	savingSourceName = $state<Partial<Record<string, boolean>>>({});
	savingQualityFallback = $state<Partial<Record<string, boolean>>>({});
	rotatingSources = $state<Partial<Record<string, boolean>>>({});
	deletingAccounts = $state<Partial<Record<string, boolean>>>({});
	lastLoadedAt = $state<number | null>(null);

	private listAbort: AbortController | null = null;
	private detailAbort: AbortController | null = null;
	private detailAccountId: string | null = null;
	private listRequestId = 0;
	private detailRequestId = 0;
	private lifecycleRevision = 0;

	async loadAccounts(options?: { silent?: boolean }) {
		const silent = options?.silent ?? false;
		const requestId = ++this.listRequestId;
		if (this.listAbort) this.listAbort.abort();
		const request = new AbortController();
		this.listAbort = request;

		if (!silent) this.loadingList = true;
		if (!silent) this.error = null;

		try {
			const api = adminSession.api;
			await this.applyAccountRead(() => api.listAccounts(request.signal), request.signal, res => {
				this.accounts = res.items.map((item) => this.mergePendingEdits(item));
				this.lastLoadedAt = Math.floor(Date.now() / 1000);
				this.error = null;
			});
		} catch (e: unknown) {
			if (requestId !== this.listRequestId || handleAuthError(e)) return;
			if (!silent) this.error = describeApiError(e);
		} finally {
			if (requestId === this.listRequestId) this.loadingList = false;
		}
	}

	async loadAccount(accountId: string, options?: { silent?: boolean }) {
		const silent = options?.silent ?? false;
		const requestId = ++this.detailRequestId;
		if (this.detailAbort) this.detailAbort.abort();
		const request = new AbortController();
		this.detailAbort = request;
		this.detailAccountId = accountId;

		if (!silent) this.loadingDetail = true;
		if (!silent) this.error = null;

		try {
			const api = adminSession.api;
			await this.applyAccountRead(() => api.getAccount(accountId, request.signal), request.signal, fresh => {
				const merged = this.mergePendingEdits(fresh);
				this.currentAccount = merged;
				this.syncAccountInList(merged);
				this.error = null;
			});
		} catch (e: unknown) {
			if (requestId !== this.detailRequestId || handleAuthError(e)) return;
			if (!silent) this.error = describeApiError(e);
		} finally {
			if (requestId === this.detailRequestId) this.loadingDetail = false;
		}
	}

	async runAccountAction(accountId: string, action: AccountAction) {
		const inFlight = this.accountActions;
		if (inFlight[accountId]) return { status: 'busy' } as const;
		inFlight[accountId] = action;
		const { api, signal } = adminSession;

		try {
			let actionError: string | undefined;
			let claimRenewal: RenewalStatus | undefined;
			try {
				if (action === 'refresh') {
					await api.refreshToken(accountId, signal);
				} else if (action === 'sync') {
					await api.syncEntitlements(accountId, signal);
				} else {
					claimRenewal = await api.claimRenewal(accountId, signal);
					if (!signal.aborted) this.patchAccountRenewal(accountId, claimRenewal);
				}
			} catch (error) {
				if (signal.aborted || handleAuthError(error)) return { status: 'redirected' } as const;
				actionError = describeApiError(error);
			}
			if (signal.aborted) return { status: 'redirected' } as const;

			try {
				await this.applyAccountRead(() => api.getAccount(accountId, signal), signal, fresh => {
					const merged = this.mergePendingEdits(fresh);
					if (this.currentAccount?.account_id === accountId) this.currentAccount = merged;
					this.syncAccountInList(merged);
				});
				if (signal.aborted) return { status: 'redirected' } as const;
			} catch (error) {
				if (signal.aborted || handleAuthError(error)) return { status: 'redirected' } as const;
				if (!actionError && !claimRenewal) return { status: 'unconfirmed' } as const;
			}

			if (actionError) return { status: 'failed', message: actionError } as const;
			return { status: 'ok', renewal: claimRenewal } as const;
		} finally {
			if (!signal.aborted) delete inFlight[accountId];
		}
	}

	setRenewalEnabled(accountId: string, enabled: boolean) {
		return this.save(
			this.savingAutoClaim, accountId,
			() => adminSession.api.updateRenewal(accountId, enabled),
			renewal => this.patchAccountRenewal(accountId, renewal)
		);
	}

	renameSource(sourceId: string, name: string | null) {
		return this.save(
			this.savingSourceName, sourceId,
			() => adminSession.api.updateSource(sourceId, { name }),
			res => this.patchSource(sourceId, src => ({
				...src, name: res.name, default_name: res.default_name, updated_at: res.updated_at
			}))
		);
	}

	setQualityFallback(sourceId: string, value: boolean) {
		return this.save(
			this.savingQualityFallback, sourceId,
			() => adminSession.api.updateSource(sourceId, { quality_fallback: value }),
			res => this.patchSource(sourceId, src => ({
				...src, quality_fallback: res.quality_fallback, updated_at: res.updated_at
			}))
		);
	}

	rotateSourceTokens(sourceId: string) {
		return this.save(
			this.rotatingSources, sourceId,
			() => adminSession.api.rotateSourceTokens(sourceId),
			res => {
				this.lifecycleRevision++;
				this.patchSource(sourceId, src => ({
					...src,
					script_url: res.script_url,
					runtime_token_preview: res.runtime_token_preview,
					updated_at: res.updated_at,
				}));
			}
		);
	}

	deleteAccount(accountId: string) {
		return this.save(
			this.deletingAccounts, accountId,
			() => adminSession.api.deleteAccount(accountId),
			() => {
				this.lifecycleRevision++;
				if (this.detailAccountId === accountId) {
					this.detailRequestId++;
					this.detailAbort?.abort();
					this.detailAbort = null;
					this.detailAccountId = null;
					this.loadingDetail = false;
				}
				if (this.currentAccount?.account_id === accountId) this.currentAccount = null;
				this.accounts = this.accounts.filter(account => account.account_id !== accountId);
			}
		);
	}

	private async save<T>(
		pending: Partial<Record<string, boolean>>,
		id: string,
		request: () => Promise<T>,
		apply: (value: T) => void
	): Promise<SaveResult<T>> {
		if (pending[id]) return { status: 'busy' };
		const { signal } = adminSession;
		pending[id] = true;
		try {
			const value = await request();
			if (signal.aborted) return { status: 'redirected' };
			apply(value);
			return { status: 'ok', value };
		} catch (error) {
			if (signal.aborted || handleAuthError(error)) return { status: 'redirected' };
			return { status: 'failed', message: describeApiError(error) };
		} finally {
			if (!signal.aborted) delete pending[id];
		}
	}

	clearError() {
		this.error = null;
	}

	reset() {
		this.listRequestId++;
		this.detailRequestId++;
		this.listAbort?.abort();
		this.detailAbort?.abort();
		this.listAbort = null;
		this.detailAbort = null;
		this.detailAccountId = null;
		this.accounts = [];
		this.currentAccount = null;
		this.loadingList = false;
		this.loadingDetail = false;
		this.error = null;
		this.accountActions = {};
		this.savingAutoClaim = {};
		this.savingSourceName = {};
		this.savingQualityFallback = {};
		this.rotatingSources = {};
		this.deletingAccounts = {};
		this.lastLoadedAt = null;
	}

	// A read started before a deletion or rotation may still hold the old account data.
	private async applyAccountRead<T>(
		fetch: () => Promise<T>,
		signal: AbortSignal,
		apply: (result: T) => void,
	) {
		while (!signal.aborted) {
			const revision = this.lifecycleRevision;
			const result = await fetch();
			if (signal.aborted) return;
			if (revision === this.lifecycleRevision) {
				apply(result);
				return;
			}
		}
	}

	private syncAccountInList(account: AccountItem) {
		const idx = this.accounts.findIndex((item) => item.account_id === account.account_id);
		if (idx === -1) return;
		this.accounts = this.accounts.map((item) =>
			item.account_id === account.account_id ? account : item
		);
	}

	private patchAccountRenewal(accountId: string, renewal: RenewalStatus) {
		if (this.currentAccount?.account_id === accountId) {
			this.currentAccount = { ...this.currentAccount, renewal };
		}
		this.accounts = this.accounts.map((item) =>
			item.account_id === accountId ? { ...item, renewal } : item
		);
	}

	private patchSource(
		sourceId: string,
		updater: (src: AccountItem['sources'][number]) => AccountItem['sources'][number]
	) {
		if (this.currentAccount) {
			const hasTarget = this.currentAccount.sources.some((s) => s.source_id === sourceId);
			if (hasTarget) {
				this.currentAccount = {
					...this.currentAccount,
					sources: this.currentAccount.sources.map((s) =>
						s.source_id === sourceId ? updater(s) : s
					),
				};
			}
		}

		this.accounts = this.accounts.map((acc) => {
			if (!acc.sources.some((s) => s.source_id === sourceId)) return acc;
			return {
				...acc,
				sources: acc.sources.map((s) => (s.source_id === sourceId ? updater(s) : s)),
			};
		});
	}

	/** 避免慢查询回填冲掉正在保存的开关或名称编辑。 */
	private mergePendingEdits(fresh: AccountItem): AccountItem {
		const existing =
			this.currentAccount?.account_id === fresh.account_id
				? this.currentAccount
				: this.accounts.find((a) => a.account_id === fresh.account_id);
		if (!existing) return fresh;

		let renewal = fresh.renewal;
		if (this.savingAutoClaim[fresh.account_id]) {
			renewal = existing.renewal;
		}

		const sources = fresh.sources.map((src) => {
			const prev = existing.sources.find((s) => s.source_id === src.source_id);
			if (!prev) return src;
			return {
				...src,
				name: this.savingSourceName[src.source_id] ? prev.name : src.name,
				default_name: this.savingSourceName[src.source_id] ? prev.default_name : src.default_name,
				script_url: this.rotatingSources[src.source_id] ? prev.script_url : src.script_url,
				runtime_token_preview: this.rotatingSources[src.source_id]
					? prev.runtime_token_preview
					: src.runtime_token_preview,
				updated_at: this.rotatingSources[src.source_id] ? prev.updated_at : src.updated_at,
				quality_fallback: this.savingQualityFallback[src.source_id]
					? prev.quality_fallback
					: src.quality_fallback,
			};
		});

		return { ...fresh, renewal, sources };
	}
}

export const accountsController = new AccountsController();
adminSession.onReset(() => accountsController.reset());

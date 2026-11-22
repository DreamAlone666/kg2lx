import { adminSession, handleAuthError } from '$lib/state/admin-session.svelte';
import type { AccountItem, RenewalStatus } from '$lib/api/types';
import { describeApiError } from '$lib/domain';

export type AccountAction = 'refresh' | 'sync' | 'claim';

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
	lastLoadedAt = $state<number | null>(null);

	private listAbort: AbortController | null = null;
	private detailAbort: AbortController | null = null;
	private listRequestId = 0;
	private detailRequestId = 0;

	async loadAccounts(options?: { silent?: boolean }) {
		const silent = options?.silent ?? false;
		const requestId = ++this.listRequestId;
		if (this.listAbort) this.listAbort.abort();
		this.listAbort = new AbortController();

		if (!silent) this.loadingList = true;
		if (!silent) this.error = null;

		try {
			const res = await adminSession.api.listAccounts(this.listAbort.signal);
			if (requestId !== this.listRequestId) return;
			this.accounts = res.items.map((item) => this.mergePendingEdits(item));
			this.lastLoadedAt = Math.floor(Date.now() / 1000);
			this.error = null;
		} catch (e: unknown) {
			if (e instanceof DOMException && e.name === 'AbortError') return;
			if (handleAuthError(e)) return;
			if (requestId !== this.listRequestId) return;
			if (!silent) this.error = describeApiError(e);
		} finally {
			if (!silent && requestId === this.listRequestId) this.loadingList = false;
		}
	}

	async loadAccount(accountId: string, options?: { silent?: boolean }) {
		const silent = options?.silent ?? false;
		const requestId = ++this.detailRequestId;
		if (this.detailAbort) this.detailAbort.abort();
		this.detailAbort = new AbortController();

		if (!silent) this.loadingDetail = true;
		if (!silent) this.error = null;

		try {
			const fresh = await adminSession.api.getAccount(accountId, this.detailAbort.signal);
			if (requestId !== this.detailRequestId) return;
			const merged = this.mergePendingEdits(fresh);
			this.currentAccount = merged;
			this.syncAccountInList(merged);
			this.error = null;
		} catch (e: unknown) {
			if (e instanceof DOMException && e.name === 'AbortError') return;
			if (handleAuthError(e)) return;
			if (requestId !== this.detailRequestId) return;
			if (!silent) this.error = describeApiError(e);
		} finally {
			if (!silent && requestId === this.detailRequestId) this.loadingDetail = false;
		}
	}

	async runAccountAction(accountId: string, action: AccountAction) {
		if (this.accountActions[accountId]) return { status: 'busy' } as const;
		this.accountActions = { ...this.accountActions, [accountId]: action };

		try {
			let actionError: string | undefined;
			let claimRenewal: RenewalStatus | undefined;

			try {
				if (action === 'refresh') {
					await adminSession.api.refreshToken(accountId);
				} else if (action === 'sync') {
					await adminSession.api.syncEntitlements(accountId);
				} else {
					claimRenewal = await adminSession.api.claimRenewal(accountId);
					this.patchAccountRenewal(accountId, claimRenewal);
				}
			} catch (error) {
				if (handleAuthError(error)) return { status: 'redirected' } as const;
				actionError = describeApiError(error);
			}

			try {
				const fresh = await adminSession.api.getAccount(accountId);
				const merged = this.mergePendingEdits(fresh);
				if (this.currentAccount?.account_id === accountId) {
					this.currentAccount = merged;
				}
				this.syncAccountInList(merged);
			} catch (error) {
				if (handleAuthError(error)) return { status: 'redirected' } as const;
				if (!actionError && !claimRenewal) return { status: 'unconfirmed' } as const;
			}

			if (actionError) {
				return { status: 'failed', message: actionError } as const;
			}
			return { status: 'ok', renewal: claimRenewal } as const;
		} finally {
			const next = { ...this.accountActions };
			delete next[accountId];
			this.accountActions = next;
		}
	}

	async setRenewalEnabled(accountId: string, enabled: boolean): Promise<'ok' | 'failed'> {
		if (this.savingAutoClaim[accountId]) return 'failed';
		this.savingAutoClaim = { ...this.savingAutoClaim, [accountId]: true };
		this.error = null;

		try {
			const renewal = await adminSession.api.updateRenewal(accountId, enabled);
			this.patchAccountRenewal(accountId, renewal);
			return 'ok';
		} catch (e: unknown) {
			if (!handleAuthError(e)) this.error = describeApiError(e);
			return 'failed';
		} finally {
			const next = { ...this.savingAutoClaim };
			delete next[accountId];
			this.savingAutoClaim = next;
		}
	}

	async renameSource(
		sourceId: string,
		name: string | null
	): Promise<{ status: 'ok'; name: string; defaultName: string } | { status: 'failed' }> {
		this.savingSourceName = { ...this.savingSourceName, [sourceId]: true };
		this.error = null;

		try {
			const res = await adminSession.api.updateSource(sourceId, { name });
			this.patchSource(sourceId, (src) => ({
				...src,
				name: res.name,
				default_name: res.default_name,
				updated_at: res.updated_at,
			}));
			return { status: 'ok', name: res.name, defaultName: res.default_name };
		} catch (e: unknown) {
			if (!handleAuthError(e)) this.error = describeApiError(e);
			return { status: 'failed' };
		} finally {
			const next = { ...this.savingSourceName };
			delete next[sourceId];
			this.savingSourceName = next;
		}
	}

	async setQualityFallback(sourceId: string, value: boolean): Promise<'ok' | 'failed'> {
		this.savingQualityFallback = { ...this.savingQualityFallback, [sourceId]: true };
		this.error = null;

		try {
			const res = await adminSession.api.updateSource(sourceId, { quality_fallback: value });
			this.patchSource(sourceId, (src) => ({
				...src,
				quality_fallback: res.quality_fallback,
				updated_at: res.updated_at,
			}));
			return 'ok';
		} catch (e: unknown) {
			if (!handleAuthError(e)) this.error = describeApiError(e);
			return 'failed';
		} finally {
			const next = { ...this.savingQualityFallback };
			delete next[sourceId];
			this.savingQualityFallback = next;
		}
	}

	clearError() {
		this.error = null;
	}

	stop() {
		if (this.listAbort) {
			this.listAbort.abort();
			this.listAbort = null;
		}
		if (this.detailAbort) {
			this.detailAbort.abort();
			this.detailAbort = null;
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
				quality_fallback: this.savingQualityFallback[src.source_id]
					? prev.quality_fallback
					: src.quality_fallback,
			};
		});

		return { ...fresh, renewal, sources };
	}
}

export const accountsController = new AccountsController();

import type { EntitlementSnapshot } from '$lib/api/types';
import { isNamedVipProduct } from './labels';

export function summarizeEntitlements(snapshot: EntitlementSnapshot) {
	const active = snapshot.entitlements?.filter((item) => item.active) ?? [];
	return {
		named: active.filter((item) => isNamedVipProduct(item.product_type)),
		hasUnnamedActive: active.some((item) => !isNamedVipProduct(item.product_type)),
		state: snapshot.entitlements_sync_failed
			? 'failed'
			: snapshot.entitlements === undefined
				? 'unknown'
				: active.length > 0
					? 'active'
					: 'empty'
	} as const;
}

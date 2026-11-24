import type { StoredEvent, SourceEvent } from '$lib/api/types';
import {
	accountLoginOutcomeLabel,
	errorCodeLabel,
	eventTypeLabel,
	labelOf,
	tokenRefreshReasonLabel,
} from './labels';
import {
	entitlementActionLabel,
	isEntitlementAlreadyDone,
	renewalOutcomeLabel,
	renewalReasonLabel,
} from './renewal';
import { requestOutcomeTone, type Tone } from './tone';
import { shortId } from './format';

/** 判断一条事件在业务上是否应视为「已完成 / 成功」而非错误。 */
export function isEventSuccess(event: SourceEvent): boolean {
	if (event.event_type === 'entitlement' && isEntitlementAlreadyDone(event.detail)) {
		return true;
	}
	return event.ok;
}

/** 状态列胶囊文案与色调（统一处理已完成、已延后与待确认状态）。 */
export function eventOutcomeBadge(event: SourceEvent): { tone: Tone; label: string } {
	if (event.event_type === 'entitlement') {
		if (isEntitlementAlreadyDone(event.detail)) {
			return { tone: 'success', label: '已完成' };
		}
		if (event.detail.outcome === 'deferred') {
			return { tone: 'info', label: '已延后' };
		}
		if (event.detail.outcome === 'uncertain') {
			return { tone: 'warning', label: '待确认' };
		}
	}
	return {
		tone: requestOutcomeTone(event.ok),
		label: event.ok ? 'OK' : 'ERR',
	};
}

/** 列表与「近期动态」共用的单行事件摘要。 */
export function describeEventHeadline(event: SourceEvent): { title: string; subtitle: string | null } {
	switch (event.event_type) {
		case 'music_url': {
			const d = event.detail;
			if (d.track_title) {
				const sub = [d.artist_name || '未知歌手', d.album_name].filter(Boolean).join(' · ');
				return { title: d.track_title, subtitle: sub };
			}
			return {
				title: '#' + shortId(d.request_hash),
				subtitle: d.album_audio_id ? d.album_audio_id : '音轨取链请求',
			};
		}
		case 'token_refresh':
			return {
				title: labelOf(eventTypeLabel, event.event_type),
				subtitle: labelOf(tokenRefreshReasonLabel, event.detail.reason),
			};
		case 'account_login':
			return {
				title: labelOf(eventTypeLabel, event.event_type),
				subtitle: labelOf(accountLoginOutcomeLabel, event.detail.outcome),
			};
		case 'entitlement': {
			const d = event.detail;
			const actionText = labelOf(entitlementActionLabel, d.action);
			const parts: string[] = [];
			if (d.reason) parts.push(labelOf(renewalReasonLabel, d.reason));
			if (d.upstream_day) parts.push(d.upstream_day);
			if (isEntitlementAlreadyDone(d)) {
				parts.push('今日已完成');
			} else if (d.outcome) {
				parts.push(labelOf(renewalOutcomeLabel, d.outcome));
			} else if (d.action === 'sync') {
				parts.push(event.ok ? '已更新上游快照' : '读取上游快照失败');
			}
			return {
				title: actionText,
				subtitle: parts.join(' · ') || null,
			};
		}
	}
}

/** 错误或诊断标签文案（已经过 131001 / 297002 非错误归一化）。 */
export function eventDiagnosticErrorLabel(event: SourceEvent): string | null {
	if (event.event_type === 'entitlement') {
		if (isEntitlementAlreadyDone(event.detail)) return null;
		if (event.detail.human_verification) return '上游需要人机验证';
	} else if (event.event_type === 'token_refresh' || event.event_type === 'music_url') {
		if (event.detail.human_verification) return '上游需要人机验证';
	}
	if (isEventSuccess(event)) return null;
	return labelOf(errorCodeLabel, event.error_code, '操作失败');
}

export function sortEventsAscending(items: StoredEvent[]): StoredEvent[] {
	return [...items].sort((a, b) => a.seq - b.seq);
}

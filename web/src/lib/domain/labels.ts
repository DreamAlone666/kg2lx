import type {
	AccountStatus,
	LoginSessionStatus,
	ProviderKind,
	RuntimeLogListItem,
} from '$lib/api/types';

/**
 * 领域词典 —— 业务状态到中文标签的**唯一事实来源**。
 *
 * 页面只负责展示，不负责「领域翻译」。此前 providerMap / statusMap / accountStatusMap
 * / stageMap 各自在路由与组件里重复定义，改一处文案需要改多处，且容易不一致；
 * 集中到此处后，组件层不再认识业务枚举字符串。
 *
 * 注意：新增枚举值时必须同步更新对应表，否则回退到原值显示（见 labelOf）。
 */

export const providerLabel: Record<ProviderKind, string> = {
	kugou_lite: '酷狗概念版',
};

export const accountStatusLabel: Record<AccountStatus, string> = {
	active: '正常',
	pending: '待处理',
	expired: '已过期',
	disabled: '已禁用',
	login_failed: '登录失败',
};

export const sessionStatusLabel: Record<LoginSessionStatus, string> = {
	pending: '准备中',
	waiting_scan: '等待扫码',
	waiting_confirm: '等待确认',
	authorized: '已授权',
	bound: '绑定成功',
	expired: '已过期',
	failed: '失败',
};

export const runtimeStageLabel: Record<RuntimeLogListItem['stage'], string> = {
	precheck: '预检',
	ensure_dfid: 'DFID',
	refresh_login: '刷新登录',
	fetch_music_url: '取链',
};

/**
 * 音质档位 —— LX 音质线上取值 → LX 官方中文档位名。
 *
 * 运行时日志里 requested_quality 存的是 LX Music 侧的**线上取值**
 * （128k / 320k / flac / flac24bit），取值域由后端白名单限定
 * （routes/runtime.rs 的 parse_music_url_request）。界面上仍显示线上取值而不是
 * 中文档位名，因为它同时是排查时要拿去 grep 的字面量，而四个中文名里最长的
 * 「Hi-Res 无损音质」在 w-28 档下根本放不下。
 *
 * 中文名抄自 LX 官方文档《音源与自定义源使用方法》的「可用音质说明」：
 *   https://www.lxmusic.cn/docs/music-source/
 *
 * 这里**只**有档位名，没有「向上游要什么质量」——后一项由服务端在运行时日志行里
 * 落盘（server/src/services/runtime.rs 的 append_log 写入时填入），前端只负责读回
 * 「那一刻实际发生了什么」。前端因此不持有那张映射表，也就不可能与服务端漂移。
 */
export const qualityLabel: Record<string, string> = {
	'128k': '标准音质',
	'320k': '高品音质',
	flac: '无损音质',
	flac24bit: 'Hi-Res 无损音质'
};

/**
 * 运行时错误码 → 中文标签。
 *
 * 错误码是写给日志的机器标识，不是写给运维的文案。`UPSTREAM_REQUEST_FAILED` /
 * `UPSTREAM_LOGIN_FAILED` / `UPSTREAM_PLAY_URL_EMPTY` 共享 `UPSTREAM_` 前缀，
 * 12px 等宽下前 9 个字符完全相同——纵向扫读时只能靠后半段区分，而每一个还要占掉
 * 约 166px 的列宽，正是「诊断列装不下、错误码被裁掉尾巴」的根因。
 *
 * 与 runtimeStageLabel 同一套做法：界面给中文，原始码通过 title 保留（见 LogsPanel），
 * 需要拿它去 grep 服务端日志时不会丢失。
 */
export const errorCodeLabel: Record<string, string> = {
	ADMIN_UNAUTHORIZED: '未授权',
	SOURCE_NOT_FOUND: '音源不存在',
	SOURCE_DISABLED: '音源已停用',
	ACCOUNT_NOT_FOUND: '账号不存在',
	ACCOUNT_DISABLED: '账号已停用',
	ACCOUNT_NOT_VIP: '账号未开通 VIP',
	LOGIN_SESSION_NOT_FOUND: '登录会话不存在',
	UPSTREAM_REQUEST_FAILED: '上游请求失败',
	UPSTREAM_LOGIN_FAILED: '上游登录失败',
	UPSTREAM_VIP_CHECK_FAILED: 'VIP 校验失败',
	UPSTREAM_PLAY_URL_EMPTY: '播放地址为空',
	INVALID_REQUEST: '参数无效',
	INTERNAL_ERROR: '内部错误',
};

/**
 * 安全的查表：后端可能先于前端新增枚举值，
 * 此时回退到原始字符串而不是渲染空白，便于用户反馈问题。
 */
export function labelOf<T extends string>(
	map: Record<T, string>,
	key: string | null | undefined,
	fallback = '未知'
): string {
	if (!key) return fallback;
	return (map as Record<string, string>)[key] ?? key;
}

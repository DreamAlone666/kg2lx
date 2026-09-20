import { isAdminApiError } from '$lib/api/errors';
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
	SOURCE_NAME_EMPTY: '标题不能为空',
	SOURCE_NAME_TOO_LONG: '标题过长',
	ACCOUNT_NOT_FOUND: '账号不存在',
	ACCOUNT_DISABLED: '账号已停用',
	ACCOUNT_NOT_VIP: '账号未开通 VIP',
	LOGIN_SESSION_NOT_FOUND: '登录会话不存在',
	UPSTREAM_REQUEST_FAILED: '上游请求失败',
	UPSTREAM_LOGIN_FAILED: '上游登录失败',
	UPSTREAM_VIP_CHECK_FAILED: 'VIP 校验失败',
	UPSTREAM_PLAY_URL_EMPTY: '播放地址为空',
	UPSTREAM_RATE_LIMITED: '上游限流',
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

/** 这几个码的中文标签只说明「哪一步坏了」，原始报文才是运维当下的线索。 */
const diagnosticErrorCodes = new Set([
	'UPSTREAM_REQUEST_FAILED',
	'UPSTREAM_LOGIN_FAILED',
	'UPSTREAM_VIP_CHECK_FAILED',
	'INTERNAL_ERROR',
]);

/**
 * 抛出的错误 → 可直接展示给中文用户的文案。
 *
 * 大多数错误码只保留中文标签，因为后端 `message` 是写给开发者的英文散文
 * （如 "source not found"），直接展示给中文用户等于没翻译；稳定机器码由后端给、
 * 用户文案由前端拥有，这是本文件的一贯分工。
 *
 * 四个**诊断类**错误码是例外：它们的中文标签只说明「哪一步坏了」，而运维当下唯一
 * 的线索恰恰是紧跟其后的原始报文（上游返回了什么、超时还是拒绝）。丢掉它等于让
 * 用户只能反过来找我们，所以这几个码保留 `中文抬头：后端 message`。
 *
 * 未知码（后端先于前端上线）回退到错误自身的 message —— 与 labelOf 同一套前向兼容
 * 思路。完全没有 `code` 的是另一种情况：AdminApiError 在响应体不是 JSON 时会自造
 * `API error {status}`，那是给开发者看的串，故换成「请求失败（HTTP xxx）」并保留
 * 状态码，而不是把它原样塞给用户。
 *
 * 已知边界：`/api/v1/runtime/music-url` 的报错不经过这里——那条路径由冻结的
 * LX Music 脚本直接消费，不经过本 UI，其文案不能靠改前端来修。
 */
export function describeApiError(error: unknown): string {
	const generic = '请求失败，请稍后重试。';

	// 非 AdminApiError（如连接页的网络失败）没有机器码可查，只能用它自己的 message。
	if (!isAdminApiError(error)) {
		return (error as Error)?.message || generic;
	}

	const label = error.code ? errorCodeLabel[error.code] : undefined;
	if (label) {
		if (error.code && diagnosticErrorCodes.has(error.code)) {
			return error.message ? `${label}：${error.message}` : label;
		}
		return label;
	}

	if (error.code) {
		// 有码但词典里没有：后端先于前端上线了新码。原样回显它的 message，
		// 便于把信息反馈回来；此时换成通用文案等于把唯一线索丢掉。
		return error.message || generic;
	}

	// 没有 code 只可能是响应体不是 JSON（正常错误信封必带 code），此时
	// AdminApiError 自造的是 `API error {status}` 这类给开发者看的串。换成中文，
	// 但保留状态码——它本身仍是有效的排查线索。
	return `请求失败（HTTP ${error.status}）`;
}

/**
 * 落盘的账号错误字段 → 可直接展示给中文用户的文案；无可展示内容时返回 null。
 *
 * 它和 describeApiError 回答的是同一个问题（这个码该说哪句中文），但数据形状不同：
 * 那边接的是当场抛出的 AdminApiError，这边接的是账号记录里**落盘后再读出**的
 * `last_error_code` / `last_error`。差别只在兜底那一步——本次改动之前写入的账号
 * 没有 code（服务端用 `#[serde(default)]` 补成 null），只剩一句已经落盘的 message。
 * 此时宁可原样显示那句散文，也好过让「最后一次错误」整块消失：它往往含上游返回的
 * 原文，是运维当下唯一的线索。这正是上一轮修接口错误信封时漏掉这个字段的原因——
 * 两者走的是不同的读取路径，不能用同一处判断覆盖。
 *
 * 其余分支与 describeApiError 同一套推理：已知的非诊断码只给中文标签（后端 message
 * 是写给开发者的英文散文，展示等于没翻译）；诊断码保留 `中文抬头：原文`，因为抬头
 * 只说「哪一步坏了」；未知码（后端先于前端上线）退回原文而非通用文案，免得把唯一
 * 线索丢掉。
 */
export function describeAccountError(code: string | null, message: string | null): string | null {
	const label = code ? errorCodeLabel[code] : undefined;
	if (label) {
		if (code && diagnosticErrorCodes.has(code)) {
			return message ? `${label}：${message}` : label;
		}
		return label;
	}

	// 「没有 code」与「有码但词典里没有」在这里合流：两种情况下原文都是仅剩的线索，
	// 原样回显。空串与 null 一视同仁——它们都没有可展示的内容，交给调用方跳过整块。
	return message || null;
}

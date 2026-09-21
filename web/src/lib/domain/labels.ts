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
 * 会员权益类型码（account.entitlements[].product_type）的**有序**白名单：最高一档在前。
 *
 * 它是这套映射的**唯一事实来源**——下面的 vipProductLabel（码 → 中文名）与
 * isNamedVipProduct（这个码是否可命名）都从它派生，中文名也写在这里而不是和次序分家。
 * 不要再另写一张表：两份白名单迟早会漂移成两套口径，而「同一个码在两页叫不同的名字、
 * 或一处上榜一处不上榜」正是本仓明令禁止的分叉。
 *
 * 依据是**与 App 会员中心的到期日逐条对齐核对过**：同一个账号下这里的每条明细都能
 * 在 App 里找到到期日相同的那一项，所以 svip / tvip 各指哪一项权益是可以确信的。
 *
 * **只放这两条**，其余码（qvip / dvip 等）刻意不在本表里编中文名：目前没有同类依据
 * 能判断它们对应哪一项权益，猜一个名字会变成一条用户无法反驳的错误断言。
 *
 * 这张表同时是权益段的白名单（见 isNamedVipProduct）：没有名字的权益位既不上榜，
 * 也不走 labelOf 的原值回退——把一个内部产品码摆给用户看，他既无法据此做任何操作，
 * 也无从判断它重不重要（同「详情页不再展示 source_id」那条取舍）。
 *
 * ---------- 次序的来源与性质（动这张表之前必读） ----------
 * 上游 `busi_vip` 的每条只暴露 is_vip / product_type / vip_begin_time / vip_end_time
 * 四个字段（见 server/src/services/kugou_lite_client.rs 的 VipBusinessEntry），**没有
 * 任何等级、权重或排序字段**；服务端还刻意保持上游返回的顺序不做排序，那边有一条断言
 * 把这件事写死了：「busi_vip 有几条就落几条，顺序与上游一致：它是并列列表，不是映射」。
 * 也就是说，[svip, tvip] 这个先后**不是从数据里推出来的**——接口对这两个码是平权的，
 * 谁前谁后都能自圆其说。
 *
 * 它成立完全是因为用户对酷狗 App「会员中心」呈现方式的观察：概念版VIP 那一档视觉更炫、
 * 权益更多，暗示它是畅听VIP 的升级档。所以将来要核对这个次序，**拿 App 会员中心去对，
 * 不要拿接口数据去「验证」它**——用一份平权的数据去证明一个排序，只会得到自己想看的答案。
 *
 * 次序目前只有一个去处：vipProductRank，也就是详情页与列表给最高一档更重的视觉强度
 * （见 tone.ts 的 vipEntitlementEmphasis）。其余消费方（labelOf / isNamedVipProduct）
 * 只看成员、不看位置。
 */
export const vipProductOrder = [
	{ productType: 'svip', label: '概念版VIP' },
	{ productType: 'tvip', label: '畅听VIP' },
] as const;

/**
 * 上面那个有序数组摊平成查表用的字典（labelOf 接的是 Record 而不是数组）。
 *
 * 这里的 `as const` 只是为了让二元组保持只读元组形状（Object.fromEntries 只接受这种
 * 形状）；**次序由 vipProductOrder 独家持有**，这里刻意不重复声明，免得两份先后各自漂移。
 */
export const vipProductLabel: Record<string, string> = Object.fromEntries(
	vipProductOrder.map((product) => [product.productType, product.label] as const)
);

/**
 * 权益码在 vipProductOrder 里的位置（0 = 最高一档）；不在白名单里的返回 **null**。
 *
 * 返回 null 而不是一个很大的数：下游要区分「不是我们认识的权益」与「只是一档较低」，
 * 把两者折叠成同一个数字，等于允许未命名的权益悄悄参与排序，而它落在哪一档纯属偶然。
 */
export function vipProductRank(productType: string | null | undefined): number | null {
	if (!productType) return null;
	const index = vipProductOrder.findIndex((product) => product.productType === productType);
	return index === -1 ? null : index;
}

/**
 * 该 product_type 是否是**我们能命名的**权益——也就是权益段该不该列它。
 *
 * 判据就是「在不在 vipProductOrder 里」，与中文名共用同一份名单（不再单独维护一张表）。
 *
 * Why 需要这条白名单：没有名字的权益位是 1 天窗口的日结条目，只要在期就永远落在
 * 「临近到期」阈值内，「即将到期」于是常亮；胶囊一旦常亮就不再是信号，而那两条真正
 * 会掉、真正需要预警的权益反而被淹掉。它们仍由接口返回（服务端不做筛选），只是不上榜，
 * 将来有需求再说。
 */
export function isNamedVipProduct(productType: string | null | undefined): boolean {
	return vipProductRank(productType) !== null;
}

/**
 * 运行时错误码 → 中文标签。
 *
 * 错误码是写给日志的机器标识，不是写给运维的文案。共享 `UPSTREAM_` 前缀的**每一个**
 * 码在 12px 等宽下前 9 个字符完全相同——纵向扫读时只能靠后半段区分，而每一个还要
 * 占掉约 166px 的列宽，正是「诊断列装不下、错误码被裁掉尾巴」的根因。所以这一族
 * 码有几个、各叫什么，本注释一概不列举：它们会增长，而上面的论证不依赖具体成员
 * （成员越多，共用前缀越浪费列宽）。要看当前全集，读下面这张表。
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
	/**
	 * 服务端**已不再产生**这个码（准入闸已移除），但必须保留。
	 *
	 * 这张表读的不是「服务端现在会抛什么」，而是**已经落盘的字符串**：运行时日志行的
	 * error_code、账号记录的 last_error_code 都可能是历史行，历史行不会因为服务端
	 * 改了行为而消失。删掉这一条，那些行会经 labelOf 回退成英文码，用户看到的是内部
	 * 标识而不是中文——排查时最需要读懂的恰恰是这些老记录。
	 */
	ACCOUNT_NOT_VIP: '账号未开通 VIP',
	LOGIN_SESSION_NOT_FOUND: '登录会话不存在',
	UPSTREAM_REQUEST_FAILED: '上游请求失败',
	UPSTREAM_LOGIN_FAILED: '上游登录失败',
	UPSTREAM_VIP_CHECK_FAILED: 'VIP 校验失败',
	/**
	 * 与冻结脚本的字典同文，不得单方面漂移：脚本在播放条上给用户看的就是这句。
	 * 它的确切含义已收窄为「没拿到 url、**且上游没有表达拒绝**」——上游明确拒绝
	 * 另有专门的码（`UPSTREAM_QUALITY_REFUSED` / `UPSTREAM_QUALITY_EXHAUSTED`），
	 * 不要再把这里写成「暂无可用音质」那类听起来像上游拒绝的说法。
	 */
	UPSTREAM_PLAY_URL_EMPTY: '未取到播放地址',
	/**
	 * 只陈述「上游明确拒绝了这一档」这个事实，不描述我们自己的降级策略。
	 *
	 * 文案与冻结脚本的字典**同文**（那条是「该音质不可用」）：同一个码会同时出现在
	 * 洛雪的播放条与这张面板上，两边读起来是两句不同的话时，用户和我们自己都无从确定
	 * 在说同一件事。上面那条注释说的规矩是**这一族标签**的规矩，不只管其中一条。
	 */
	UPSTREAM_QUALITY_REFUSED: '该音质不可用',
	/** 降级已走到底、每一档都被明确拒绝。同样与脚本字典同文（「所有音质均不可用」）。 */
	UPSTREAM_QUALITY_EXHAUSTED: '所有音质均不可用',
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

/**
 * 诊断类错误码：中文标签只说明「哪一步坏了」，原始报文才是运维当下的线索。
 *
 * 判据是**这个码的中文标签是否已经把整个状态说完了**，而不是「是不是上游相关」。
 * 例如「该音质被上游拒绝」本身就完整描述了状态，背后的英文 message 只是同一件事的
 * 另一种说法，拼上去是重复；反过来「上游请求失败」没说是超时、拒绝还是连不上，
 * 那份报文才是唯一线索。新增码时按这条判据决定要不要进这个集合，不要按前缀归类。
 */
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
 * **诊断类**错误码（见 diagnosticErrorCodes）是例外：它们的中文标签只说明「哪一步
 * 坏了」，而运维当下唯一的线索恰恰是紧跟其后的原始报文（上游返回了什么、超时还是
 * 拒绝）。丢掉它等于让用户只能反过来找我们，所以这几个码保留 `中文抬头：后端 message`。
 *
 * 未知码（后端先于前端上线）回退到错误自身的 message —— 与 labelOf 同一套前向兼容
 * 思路。完全没有 `code` 的是另一种情况：AdminApiError 在响应体不是本仓的错误信封
 * 时会自造一个给开发者看的串，故换成一句说明「响应不是接口数据」的中文并保留状态码
 * （状态码本身仍是排查线索），而不是把它原样塞给用户。
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

	// 没有 code 只可能是响应体不是本仓的错误信封（正常错误信封必带 code），此时
	// AdminApiError 自造的是给开发者看的串。换成中文，并保留状态码——它本身仍是
	// 有效的排查线索。
	return `服务器返回的不是接口数据（HTTP ${error.status}）`;
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

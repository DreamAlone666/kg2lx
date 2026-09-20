<script lang="ts">
	import { untrack } from 'svelte';
	import { Button, Card, CardContent, CardHeader, Skeleton, StatusBadge } from '$lib/components/ui';
	import { sourceLogsController } from '$lib/features/source-logs/controller.svelte';
	import { usePageVisible } from '$lib/hooks/use-page-visible.svelte';
	import {
		errorCodeLabel,
		formatClockTime,
		formatDateTime,
		formatLatency,
		formatLogTime,
		labelOf,
		qualityLabel,
		qualityTone,
		requestOutcomeTone,
		runtimeStageLabel,
		shortId
	} from '$lib/domain';
	import type { RuntimeLogListItem, RuntimeLogView } from '$lib/api/types';
	import { EASE_OUT_SOFT } from '$lib/utils/motion';
	import { cn } from '$lib/utils.js';

	let { sourceId }: { sourceId: string } = $props();

	/** 用于让指示器诚实反映「轮询已暂停」，而不是继续显示「实时」。 */
	const pageVisible = usePageVisible();

	const views: { value: RuntimeLogView; label: string }[] = [
		{ value: 'all', label: '全部' },
		{ value: 'errors', label: '错误' }
	];

	const skeletonSlots = [0, 1, 2, 3];

/**
 * 「错误」视图下是否还需要状态列。
 *
 * 该视图已经保证每一行都是失败，红色的 ERR 胶囊在其中是零信息——它占着 80px
 * 却只是在重复筛选条件。整列让出去，腾出的宽度交给该档的布局算法自动分配
 * （见 hasSongMeta）。
 */
const showOutcome = $derived(sourceLogsController.view !== 'errors');

/**
 * 本页是否有任何一行真的带上了歌曲元数据。它决定整个表格用哪种布局算法。
 *
 * **有元数据** —— `w-full table-fixed`：歌曲列是自由文本（歌名可长可短），必须
 * 靠 table-fixed 钉死宽度才能安全 truncate；其余列按内容上限定档，剩余空间全部
 * 给歌曲列——因为这时只有它是"message 列"。
 *
 * **无元数据** —— `w-full table-auto`，且不给任何列定宽：非歌曲列全是定长 token，
 * 没有 message 列。交给浏览器的自动表格算法，它会**按各列内容的宽度比例**把剩余
 * 宽度摊掉。表仍然铺满卡片，而每列的富余量都正比于它自己的内容——不会出现"某一
 * 列 95% 是空气"。
 *
 * 为什么这是第三次才对。前两次都记在这里，别再走一遍：
 *
 *   ① 余量全派给歌曲列 → 558px 里放 72px 的「无歌曲元数据」，行中被掏出一个约
 *      470px 的空洞。
 *   ② 余量全派给诊断列 → 256 → 622px 里放 30px 的「取链」，空洞从行中挪到了行尾，
 *      那一列仍是 95% 空气。
 *   ③ 干脆让表缩到内容宽度（去掉 w-full）→ 表只有 628px，卡片右半边全空。
 *
 * ① ② 的病根是**把余量派给某一列**：Carbon 的数据表规范只说「列宽随内容变化，
 * 列间只需最小间距」，没有哪个设计体系支持把余量塞进某一列；NN/g 更明确反对把
 * 元数据组右锚、中间留死白——「相邻数据点易于比较」正是表格的价值所在。
 * ③ 的病根是**把余量推出表外**：卡片里的空白没有任何东西去界定它，就只是白。
 *
 * 正解是让余量**按内容比例进到每一列**，也就是 table-auto + w-full 的原生行为，
 * 不需要任何写死的百分比。（把宽度均分到每一列也试过：能铺满，但会把歌曲列压到
 * 1024px 下的约 200px——有元数据时歌名会被截成十来个字，所以那个方案绝不能
 * 用在有元数据档。）
 *
 * 为什么必须写成分支：音源脚本 0.1.1 只上报 hash / album_audio_id / quality，
 * track_title 从未落盘过，实测 20 行窗口跨 2026-05 ~ 2026-09 全是 null。这不是
 * 过渡态——后端补上元数据只影响此后新产生的行，历史行永远是 null。所以「无元
 * 数据」是长期常态，布局必须对它成立。
 *
 * 为什么是**本页级**开关：列宽是表级属性。代价是轮询到第一条带元数据的日志时会
 * 整体切一次；这是预期的一次性行为，不是抖动。
 */
const hasSongMeta = $derived(sourceLogsController.logs.some((log) => log.track_title));

/**
 * 本页是否有任何一行真的带上了上游观测。它决定「上游」列整列是否出现。
 *
 * 与 hasSongMeta 同一套「本页级开关」的做法，但理由更硬：**没有任何观测的行是常态**。
 * 服务端只在 song/url 那次调用上记录观测（预检、刷新登录、DFID 都不经过那次调用），
 * 而本次改动之前落盘的历史行则全部没有这一组字段（后端 `#[serde(default)]` 补成 null）。
 * 若不做整列让出，一页历史数据里就会常驻一列永久空白——正是「用无效内容凑版式」，
 * 同一个教训见「错误」视图里被折叠的状态列。
 *
 * 代价与 hasSongMeta 相同：轮询到第一条带观测的日志时会整体切一次，是预期的一次性行为。
 */
const hasUpstreamStatus = $derived(
	sourceLogsController.logs.some((log) => log.upstream_status !== null)
);

/**
 * 表头单元格的基础样式。
 *
 * 分隔线用 `inset box-shadow` 而不是 `border-b`：`border-collapse: collapse` 下
 * 单元格的边框由**表格**绘制，不会跟着 sticky 的 thead 一起移动——实测滚到一半时
 * 表头与第一行数据之间完全没有线（用红色 3px border 探针复现：红线不出现在可见区），
 * 表头于是与数据糊成一片。换成 inset 阴影后红线精确落回吸顶位置。
 *
 * 用满强度的 `--color-border` 而非行分隔的 70%：表头是分区而不是行，略重的线
 * 反而符合分区语义。
 */
const TH = 'px-4 py-2.5 font-medium shadow-[inset_0_-1px_0_0_var(--color-border)]';

	/** 新行高亮：关键帧定义在 layout.css，以内联 animation 引用以免被作用域样式裁剪。 */
	const ROW_FLASH = `kg2lx-row-flash 900ms ${EASE_OUT_SOFT}`;

	function rowFlashStyle(isNew: boolean) {
		return isNew ? `animation: ${ROW_FLASH}` : undefined;
	}

	/**
	 * 耗时列：0 不是「很快」，而是「没测到」。
	 *
	 * 服务端在「失败发生在发出请求之前」的各条路径（预检、ensure_dfid 失败、refresh_login
	 * 失败）写的是占位 0，而真实的上游耗时也会向下取整成 0。渲染成 `0ms` 会被读作「极快」，
	 * 还会与成功行的 2ms 混为一谈——一个没有被测量过的值被显示成了测量结果。
	 * 统一显示 `—`：既不为未测量的值背书，也不丢掉 30ms / 86ms 这类真实测量。
	 */
	function latencyText(log: RuntimeLogListItem): string {
		return log.latency_ms > 0 ? formatLatency(log.latency_ms) : '—';
	}

	function latencyTitle(log: RuntimeLogListItem): string | undefined {
		return log.latency_ms > 0 ? undefined : '未记录到耗时';
	}

	/**
	 * 上游观测列的文本：`业务 status / priv_status`，形如 `2/0`；本行没有观测时为 `—`。
	 *
	 * 两个数字合成一格而不是各占一列：它们是一对（「明确拒绝」正是 `2/0` 这个组合，
	 * priv_status 单独看没有意义），而列宽是有限的——诊断列那三次失败的尝试已经
	 * 得出「信息要按列宽分配而不是按重要性堆放」。
	 *
	 * **不在前端给这两个数字起名字**（不写「明确拒绝」之类）。它们的含义属于服务端的
	 * 状态体系，界面上那句中文由诊断列的失败胶囊承担、文案来自 labels.ts；在这里再翻
	 * 一遍就是同一件事的第二份词典，改一处必漏另一处——本仓库已有过记录。
	 * 空值显示 `—` 而不是留白，与耗时列同一条理由：留白分不清「没有」和「没记到」。
	 */
	function upstreamText(log: RuntimeLogListItem): string {
		if (log.upstream_status === null) return '—';
		return log.upstream_priv_status === null
			? `${log.upstream_status}`
			: `${log.upstream_status}/${log.upstream_priv_status}`;
	}

	/**
	 * 上游观测列的悬停说明。只回答「这两个数字是什么、为什么没有」。
	 *
	 * 没有值的成因不止一种，与 qualityTitle 里 upstream_quality 那段完全同源：
	 * ① 本行描述的不是 song/url 那次取链（预检 / 刷新登录 / DFID 都不经过它，
	 * 服务端据此不记观测）；② 该行产生于服务端开始记录上游观测之前。两种都要说出来，
	 * 否则用户会以为「预检行没有观测」是记录丢了。
	 */
	function upstreamTitle(log: RuntimeLogListItem): string {
		if (log.upstream_status === null) {
			return '本行没有上游观测：不是 song/url 那次取链（服务端只在那次调用上记录），或该行产生于开始记录之前';
		}
		const parts = [`上游返回的业务状态 ${log.upstream_status}`];
		if (log.upstream_priv_status !== null) parts.push(`priv_status ${log.upstream_priv_status}`);
		parts.push('上游自己给的值，不是本服务的判断结果');
		return parts.join('｜');
	}

	/**
	 * 音质胶囊的悬停详情：线上取值 → LX 官方档位名 →（发生了降级时）本次实际要的档位 →
	 *（该行记录到时）向上游要的质量参数 →（上游返回了文件时）格式与字节数 →
	 *（客户端上报过时）歌曲档位元数据。
	 *
	 * 顺序就是排查时的心智顺序：先是你在日志里看到、要能直接复制去 grep 的那些字面量，
	 * 紧接着把它们翻成人话（四档里只有 flac24bit → high 是真正的重命名，而它常常正是
	 * 失败与成功的分界），再往后依次收紧到「这一次到底要了什么、又真的拿到了什么」。
	 * 每一段都不是装饰，但它们**各有一条独立的出现条件**，见下面逐个说明。
	 *
	 * 从「向酷狗请求的质量参数」往后，每一段都直接读日志行落盘的值，**不在前端推导**：
	 * 它们是发生过的事实，该由服务端记（写日志时填入，见 server/src/services/runtime.rs
	 * 的 append_log）。前端曾经复制过一张映射表，那等于把服务端逻辑的影子副本放进界面
	 * ——服务端一改，界面只会安静地说错话。
	 *
	 * 各段为什么有时不出现：
	 *
	 * - `upstream_quality` 为空时整段不显示。成因不止一种：① 本行不是 `song/url` 那次
	 *   调用（预检 / 刷新登录 / DFID 都不带质量参数，服务端据此不记，见 runtime.rs 的
	 *   upstream_quality_for）；② 该行产生于服务端开始记录之前。
	 * - `attempted_quality` 只在**与 requested_quality 不同**时才出现。降级后的那一行里
	 *   requested_quality 仍是用户当初要的档位（这是事实，不该改写），要看出「本次要的
	 *   是另一档」只能靠它；两者相等时它等于把第一段再说一遍，故跳过。
	 * - `upstream_ext_name` / `upstream_file_size` 只在**上游真的返回了一个文件**时才出现，
	 *   而它们是唯一能看出静默回落的地方：上游对没有该声部的歌仍然返回成功，只是把 128k
	 *   的 mp3 给你（文件大小与 `quality=128` 完全相同）。不落这两个字段，界面只能显示
	 *   一个「成功」。
	 * - `reported_types` 只在客户端上报过时才出现，原样呈现供比对。它是一个**记录**：
	 *   用来对照「客户端以为有什么」与「上游实际给了什么」，界面上任何分支都不得读它。
	 *
	 * 这里**不用** labelOf：它在查表未命中时回退的是原值本身（见 labels.ts），而这个原值
	 * 已经是本 tooltip 的第一段，回退过去会渲染成 `hires｜hires`。labelOf 的那个行为对
	 * 错误码是对的（原值不在别处出现、必须露出来），在这里则只制造重复。
	 *
	 * 之所以挂在胶囊上而不是表头：胶囊就是「看不懂的那个值」本身，视线本来停在
	 * 那里；表头没有任何悬停提示，挂上去的 tooltip 发现率接近零，等于没写。
	 *
	 * 顺带一提，这里原本的 title 是 `log.requested_quality` —— 与胶囊上可见的文字
	 * 完全相同，是一个零信息的 tooltip，换掉不损失任何东西。
	 */
	function qualityTitle(log: RuntimeLogListItem): string {
		const q = log.requested_quality;
		const label = qualityLabel[q] ?? '前端未收录的音质档位（可能后端新增了取值）';
		const parts = [q, label];

		if (log.attempted_quality && log.attempted_quality !== q) {
			parts.push(`本次实际向上游请求的档位：${log.attempted_quality}`);
		}
		if (log.upstream_quality) parts.push(`向酷狗请求的质量参数：${log.upstream_quality}`);

		// 判据是「字段有没有值」而不是「本行 ok 是否为真」：这两个字段只在上游真的返回了
		// 一个文件时才落盘，按字段取值呈现就不会出现「界面断言成功、字段却没有值」的分叉。
		if (log.upstream_ext_name !== null || log.upstream_file_size !== null) {
			const ext = log.upstream_ext_name ?? '格式未记录';
			const size =
				log.upstream_file_size !== null ? `${log.upstream_file_size} 字节` : '大小未记录';
			parts.push(`上游实际返回：${ext}，${size}`);
		}

		if (log.reported_types !== null) {
			parts.push(`客户端上报的歌曲档位元数据：${JSON.stringify(log.reported_types)}`);
		}

		return parts.join('｜');
	}

	/**
	 * 失败胶囊的悬停详情：失败的那一段上游调用 + 原始错误码。
	 *
	 * 胶囊上只放中文标签，机器可读的原文必须还能拿到——排查时要拿它去 grep
	 * 服务端与 KuGouMusicApi 的日志，而「中文标签」是给人读的中间层，不能成为
	 * 唯一出口。
	 *
	 * 这里放 upstream_endpoint 而不是 stage 的中文标签：在**真的调用过上游**的落盘
	 * 点上两者一一对应（FetchMusicUrl ⟺ "song/url"、RefreshLogin ⟺ "refresh_login"、
	 * EnsureDfid ⟺ "ensure_dfid"），而 endpoint 是**能直接拿去 grep 的那个字符串**，
	 * 中文标签做不到。冗余字段放进 title 是零布局成本的——这正是它该待的地方，
	 * 而不是在诊断列里再占一格。
	 *
	 * 预检的落盘点**故意**不在这个对应关系里，这也是本函数的 tooltip 必须带条件的
	 * 原因：预检（`LogErrorContext::precheck()`）把 upstream_endpoint 写成空串——音源
	 * 停用 / 账号不存在 / 账号禁用 / VIP 失效都发生在任何上游调用之前，给它们编一个
	 * 端点就是发明一次没发生过的调用（同一条纪律见 runtime.rs 的 upstream_quality_for：
	 * 只有 song/url 那次调用才记质量参数）。有了 `if (log.upstream_endpoint)` 这一兜，
	 * 预检行的 tooltip 退化成只剩「错误码 XXX」，而不是渲染出一句「上游端点 」悬着。
	 *
	 * 于是落盘点总数与带端点的落盘点数不再相等，而两个数都随每次新增落盘点而变
	 * ——此前已经错过一次（预检落盘加入时整体变了），别裸抄常量。要复核就现场跑，
	 * 并把范围钉在真正落盘的那个文件上：
	 *
	 *   grep -cE 'append_log\(AppendLogInput|\.log_and_err\(' server/src/services/runtime.rs
	 *     → 落盘点总数
	 *   grep -c 'LogErrorContext::precheck(' server/src/services/runtime.rs
	 *     → 预检落盘点数 + 1（正文里那处 `LogErrorContext::precheck()` 的提及也匹配；
	 *        `fn precheck` 的定义不带类型名前缀，不匹配）
	 *
	 * 两条都必须限定在 runtime.rs 内：整树 grep 还会数进 domain/runtime_log.rs 里那个
	 * 造 RuntimeLog 的 fixture。两者相减才是带端点的落盘点数。
	 *
	 * 刻意按**调用点形状**匹配、且不写任何具体数字：数字每加一个落盘点就过期一次（已经
	 * 过期过一次），形状不会。precheck 那条原先写的是 `'LogErrorContext::precheck(),'`，
	 * 靠尾逗号把定义与散文里的提及排除掉——调用点为了带上请求 id 改成
	 * `precheck(&client_request_id)` 之后，它恒为 0，而 0 看上去很像「一个预检落盘都没有」。
	 * 凡是靠上下文凑出来的模式都会这样烂掉：宁可多带一个 `+1` 的修正项。
	 *
	 * 这条 tooltip 里新加的上游观测（status / priv_status / fail_process）与端点同一个
	 * 来源纪律：**有才拼**。它们同样只在上游真的被调用过、且服务端已开始记录时才存在；
	 * 预检行与历史行一律什么都没有，而不是渲染出一串空值。
	 *
	 * 不再拼接 log.error：它在服务端被 sanitize_runtime_log_error 无差别地塌成同一个
	 * 常量（刻意不复述那个字面量），胶囊上的中文标签已经表达了同一件事，拼进来只是重复。
	 */
	function failureDetail(log: RuntimeLogListItem): string {
		const parts: string[] = [];
		if (log.upstream_endpoint) parts.push(`上游端点 ${log.upstream_endpoint}`);
		if (log.error_code) parts.push(`错误码 ${log.error_code}`);

		// 上游自己给的观测：这是本次改动之前查不到、只能人肉重放上游请求才能拿到的部分。
		if (log.upstream_status !== null) {
			const observation = [`上游 status=${log.upstream_status}`];
			if (log.upstream_priv_status !== null) {
				observation.push(`priv_status=${log.upstream_priv_status}`);
			}
			parts.push(observation.join(' '));
		}
		// fail_process **原样**呈现，不做任何解读：实测同一账号同一小时内对同一首歌
		// 都见过 ["pkg","buy"] / ["buy"] / [] 三种取值，任何据它下的结论下次就会失效。
		// 判空用 `!== null` 而不是真值判断，否则空数组与「没记录」在界面上无从区分，
		// 而「上游这次没给 fail_process」本身就是一个观测结果。
		if (log.upstream_fail_process !== null) {
			parts.push(`上游 fail_process=${JSON.stringify(log.upstream_fail_process)}`);
		}

		return parts.join('｜') || '请求失败';
	}

	/**
	 * 「降级 N」标记的悬停说明。
	 *
	 * 这枚标记是**必需**的，不是装饰：一次「降级后成功」的播放会写**两行**——被拒的
	 * 那一档 ok=false、成功的那一档 ok=true。少了它，被拒的那一行在「错误」视图里与
	 * 一次纯粹的失败长得一模一样，用户会以为自己遇到了两次故障，而其实只有一步。
	 *
	 * 说明里点出字段名 client_request_id 而不是只复述「两行是同一次请求」：配对关系
	 * 就靠它，而这个 id 不占任何一列的宽度，别处拿不到这个线索。
	 */
	function fallbackTitle(log: RuntimeLogListItem): string {
		const parts = [
			`已比用户请求的档位（${log.requested_quality}）向下改 ${log.quality_fallback_index} 档`,
		];
		if (log.attempted_quality) parts.push(`本次向上游要的档位：${log.attempted_quality}`);
		parts.push('一次降级成功的播放会写两行：被拒的那一档 ok=false、成功的那一档 ok=true');
		if (log.client_request_id) parts.push(`两行共用同一个请求关联 id：${log.client_request_id}`);
		return parts.join('｜');
	}



	// untrack：轮询启停会写入 controller 状态，不应成为本 effect 的依赖而自触发
	$effect(() => {
		if (!sourceId) return;
		untrack(() => sourceLogsController.startPolling(sourceId));
		return () => untrack(() => sourceLogsController.stopPolling());
	});
</script>

{#snippet songCell(log: RuntimeLogListItem)}
	{#if log.track_title}
		<span class="block truncate font-medium text-foreground">{log.track_title}</span>
		<!--
			专辑名不再叠 opacity-60。它把 --color-muted-foreground (#52525b) 在卡片白底上
			的对比度从实测 7.73:1 压到 2.90:1，低于 WCAG 正文 4.5:1 的下限（12px 小字
			更该守住这条线）。层级已经由 `·` 分隔符和「标题在上、歌手专辑在下」的
			位置关系表达了，不需要再靠降对比度。
		-->
		<span class="block truncate text-muted-foreground">
			{log.artist_name || '未知歌手'}{#if log.album_name}<span>· {log.album_name}</span>{/if}
		</span>
	{:else}
		<!--
			没有元数据时，这一格不该用一整行去说「无歌曲元数据」——那是一句占着
			列宽却零信息的常量，同一个教训见状态列的 ERR 胶囊。

			改放这首歌真正能用的**身份**：酷狗歌曲 hash。它本来就存在，只是此前
			被放进了诊断列、还顶着「请求指纹」这个错误的名字（见 diagMeta 的注释）。
			搬过来是一箭双雕：歌曲列有了内容，诊断列少一个 60px 的等宽片段。

			一个容易误判的点：失败行（61px）与成功行（41px）的高度落差是**结构性**的，
			不归这一格管——failureBadge 始终独占诊断列的一行（该列是 flex-col）。
			移走 hash 只把失败行从 65px 带到 61px（那 4px 是它此前蹭掉的胶囊内边距），
			不会把它们压回 41px。

			短形态（7 位）刻意保留：截断是为了宽度，完整 32 位在 title 里，需要拿去
			核对时不会丢。7 位足以肉眼比对重复（实测 20 行里 #1A3CAAC 出现 4 次）。
		-->
		<span
			class="block truncate font-mono text-foreground"
			title={`歌曲 hash ${log.request_hash}｜音源脚本未上报歌曲名，暂以 hash 标识该歌曲`}
		>
			#{shortId(log.request_hash)}
		</span>
		{#if log.album_audio_id}
			<!--
				这一格是个"裸数字"，界面上没有任何东西说明它是什么。它此前也一样裸着
				（挂在「无歌曲元数据」下面），只是没人在意——现在它成了行身份的一部分，
				必须有解释出口，否则就是一个只能靠猜的字段。

				它是取链时一并透传给上游的专辑音频 ID（song/url 的 album_audio_id 参数），
				和 hash 一起构成这次请求的完整定位。归 tooltip，不占列宽。
			-->
			<span
				class="block truncate font-mono text-caption text-muted-foreground"
				title={`album_audio_id ${log.album_audio_id}｜取链时与 hash 一并透传给上游的专辑音频 ID`}
			>
				{log.album_audio_id}
			</span>
		{/if}
	{/if}
{/snippet}

{#snippet qualityCell(log: RuntimeLogListItem)}
	<!--
		max-w-full + truncate 的**实际**作用是「不让一个过长的音质串顶出列边界、
		压到邻列」，这一点成立，防御保留。

		但不要指望这里出现省略号：StatusBadge 的根是 inline-flex，文本会变成匿名
		flex item，而 text-overflow 不是继承属性——它拿到的是初始值 clip，于是超长
		时是被**裁掉**而不是显示 `…`。（此前注释写的是「会显示省略号」，与实现不符。）

		不改的原因：requested_quality 在后端是被白名单校验过的闭集
		（128k / 320k / flac / flac24bit，见 routes/runtime.rs 的 parse_music_url_request），
		最长取值 flac24bit 在 w-28 档下也放得下，裁切路径在真实取值域内不可达。
		为了一个不可达路径给全站共用的 StatusBadge 换布局模型不值得。
		title 仍保留完整取值，信息不丢失。
	-->
	<StatusBadge
		tone={qualityTone(log.requested_quality)}
		class="max-w-full truncate font-mono"
		title={qualityTitle(log)}
	>
		{log.requested_quality}
	</StatusBadge>
{/snippet}

<!--
	状态列只放「成功 / 失败」两个短标签。

	错误码曾经直接塞进这个胶囊，但 UPSTREAM_REQUEST_FAILED 这类长码需要约 138px，
	远超该列 80px（w-20）的宽度；胶囊被压缩后文字会溢出到相邻的时间列，造成重叠。
	教训是信息要按**列宽**分配而不是按重要性堆放：错误码与错误原因因此移到
	更宽的「诊断」列，状态列只保留可一眼扫读的成败。
-->
{#snippet outcomeBadge(log: RuntimeLogListItem)}
	<StatusBadge tone={requestOutcomeTone(log.ok)} class="font-mono">
		{log.ok ? 'OK' : 'ERR'}
	</StatusBadge>
{/snippet}

<!--
	「上游」列：上游自己给的两个业务状态数字（业务 status / priv_status），形如 `2/0`。

	为什么值得独占一列：这两个数字此前在服务端就被丢掉了，排查「上游到底说了什么」只能
	人肉重放上游请求。它紧挨着「状态」列（我们的成败）摆放，构成「上游怎么看 / 我们怎么办」
	的对照——正是「相邻数据点易于比较」这条表格价值的用例（见 hasSongMeta 的引述）。

	为什么不给这两个数字起中文名：见 upstreamText 的注释，那会变成第二份词典。

	为什么整列可能不出现：见 hasUpstreamStatus。
-->
{#snippet upstreamCell(log: RuntimeLogListItem)}
	<span class="tabular font-mono" title={upstreamTitle(log)}>{upstreamText(log)}</span>
{/snippet}

{#snippet diagMeta(log: RuntimeLogListItem)}
	<!--
		只放「这次请求是怎么发生的」：阶段，刷新与重试这两个过程标记，本行在降级阶梯
		上的位置，以及失败行才有的上游 HTTP 状态。成败与错误已由状态列和 failureBadge
		承担，不在这里重复。

		三条纪律：

		1. 阶段不再套胶囊。每行一枚 40~56px 的圆角块只为了说「取链」，是这一列最贵
		   的形态。改成加重的纯文本后，它仍是本格最左、最深的锚点，扫读顺序不变。

		2. 歌曲 hash 不在这里。它是**这首歌是谁**，不是这次请求怎么了——此前它被
		   叫做「请求指纹」，还附了一句「相同指纹表示两次请求的内容完全相同」，而
		   那句是错的：同一首歌换音质取链，hash 相同而请求体不同。现在它回到歌曲列
		   并改回本名（见 songCell）。

		3. 上游观测的两个数字也不在这里，「上游」列已经承担了它。同一件事在诊断列再
		   出现一次，正是「同一次失败被表达三遍」的老毛病。

		上游端点（song/url / refresh_login / ensure_dfid）刻意不进来：在带端点的行上
		它与 stage 一一对应，放进布局只是再加一个零信息元素；预检行压根没有端点，
		就更没什么可放。它是 title 的原料，见 failureDetail。
	-->
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-caption text-muted-foreground">
		<span class="font-medium text-foreground">{labelOf(runtimeStageLabel, log.stage)}</span>
		<!--
			「降级 N」：本行不是用户当初要的那一档，而是阶梯上向下第 N 次尝试。

			必须常驻可见，不能只进 tooltip——**一次降级后成功的播放会写两行**（被拒的
			那一档 ok=false、成功的那一档 ok=true），在「错误」视图里那一行 ok=false 的
			中间档看起来就是一次普通失败。有了这枚标记，两行才读得出「这是一步」。

			用 warning 而不是 danger：这是**策略按预期工作**留下的痕迹，不是故障；
			与紧邻的「已刷新 Token」「重试 N」同档，三者都是「过程发生了什么」。
		-->
		{#if log.quality_fallback_index > 0}
			<span class="font-medium text-warning" title={fallbackTitle(log)}>
				降级 {log.quality_fallback_index}
			</span>
		{/if}
		<!--
			「已刷新 Token」只在 stage 不是 refresh_login 时显示。两个理由，都是实测的：

			① 冗余：stage=refresh_login 时 refresh_attempted 恒为 true。187 行线上日志里
			   refresh_login 出现 5 次，refresh_attempted 全是 true，零反例；代码上也是必然
			   ——runtime.rs 里两处 stage=RefreshLogin 的落盘点，都挂在
			   `if let Err(e) = self.do_refresh(&mut account).await {` 的 Err 分支上
			   （grep 这个 `if let` 恰好两处命中），也就是说这条日志只在刷新失败时才产生。
			   既然「这次就是刷新那一步」已经由左边的阶段标签说了，再挂一枚「已刷新
			   Token」不增加任何信息。

			② 更糟的是它会变成假话：字段是 refresh_attempted（**试图**），文案却是「已刷新」
			   （**完成**）。在 refresh_login 那些行上刷新按定义就是失败的那个——同一格会渲染成
			   「刷新登录 · 已刷新 Token · 上游登录失败」，同时宣称 token 刷新过了、又刷新失败了。

			而在 stage=fetch_music_url 的行上它是对的：刷新失败会提前 return，能走到取链就说明
			刷新成功了（线上那 3 行正是如此）。那里的「已刷新 Token」解释了这一行为什么慢，
			是有效信息，保留。

			不用 `refresh_attempted && ok` 代替这个条件：取链重试成功后才拿到空 url 的行是
			ok=false 但刷新确实成功过，那样会把有效信息也一起藏掉。
		-->
		{#if log.refresh_attempted && log.stage !== 'refresh_login'}
			<span class="font-medium text-warning">已刷新 Token</span>
		{/if}
		{#if log.retry_count > 0}
			<span class="font-medium text-warning">重试 {log.retry_count}</span>
		{/if}
		<!--
			status_code 只在失败且非空时显示。成功行恒为 200，逐行渲染就是又一个
			零信息常量（同一个教训见「错误」视图折叠状态列）；而失败且有值时它恰恰
			是关键线索——例如上游返 200 却没有返回 url（runtime.rs 的 empty url 分支）。
		-->
		{#if !log.ok && log.status_code !== null}
			<span class="tabular font-mono">HTTP {log.status_code}</span>
		{/if}
	</div>
{/snippet}

{#snippet failureBadge(log: RuntimeLogListItem)}
	<!--
		失败的唯一主表达，只出现一次。同一次失败此前被表达三遍（状态列 ERR、
		诊断列错误码胶囊、诊断列错误原因），既冗余又把诊断列撑成 2~3 行。

		现在收敛成一枚胶囊，两个字段合一：

		- `error` 不再单独占一行。它在服务端已被 sanitize 成常量
		  （runtime_log.rs 的 sanitize_runtime_log_error 不看原因、一律写同一个值；
		  **刻意不复述那个字面量**，免得常量一改名这半句就变成假话），而它非 null
		  意味着 {#if log.error} 永远成立，于是每个失败行都被一个常量吃掉整整一行。
		  真实原因仍进 title。
		- 错误码换成中文标签（原始码进 title）。`UPSTREAM_*` 这一族码在前 9 个字符上
		  完全相同，却各要 166px；换成「上游请求失败」后只要约 84px。刻意不写这一族
		  有几个码——本轮刚加了一个，写死的数量下一次改动就会变成假话，而论证本身
		  不依赖它（码越多，共用前缀越浪费列宽）。

		顺带删掉一条死分支：此前是 flex-wrap + 原因 min-w-24(96px)，号称「装得下时
		同行」。实测等宽字符 6.71px、诊断列内容宽 192px，同行需要
		6.71n + 12 + 6 + 96 ≤ 192，即错误码不超过 11 字符；而全部错误码里最短的
		INTERNAL_ERROR 也有 14 字符。也就是说这条分支在真实取值域内一次都不会命中，
		调宽度是徒劳的——问题出在「用一列同时承载机器码与人读文案」这个前提上。
	-->
	<span
		class="rounded bg-danger-soft px-1.5 py-0.5 text-caption text-danger"
		title={failureDetail(log)}
	>
		{labelOf(errorCodeLabel, log.error_code, '请求失败')}
	</span>
{/snippet}

<Card class="mt-6 overflow-hidden">
	<CardHeader class="gap-0 border-b border-border/70">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
				<h2 class="text-subheading" id="runtime-logs-heading">运行时日志</h2>

				<!--
					顺序即优先级，不得随意调换：暂停压过一切，其次是失败，再次是进行中的拉取。
					error 分支是补上的——此前轮询是静默拉取（轮询期间 loading 恒为 false），
					于是一次瞬时失败之后徽章仍显示绿色「实时」，与紧邻的「日志刷新失败」
					同屏矛盾：徽章在替一个已经失败的状态背书。
					（首屏那次拉取不静默、loading 会亮，但那是「加载中」而不是「刷新失败」，
					不归本分支管。）

					「实时」刻意不再脉冲。它是长期驻留的被动状态，用户无法据此做任何动作，
					无限循环动画只是持续的分心源；脉冲只留给用户正在等待其结束的状态
					（如「刷新中」与扫码页的「等待扫码」）。
				-->
				{#if !pageVisible.visible || sourceLogsController.paused}
					<StatusBadge tone="neutral" dot>已暂停</StatusBadge>
				{:else if sourceLogsController.error}
					<StatusBadge tone="danger" dot>刷新失败</StatusBadge>
				{:else if sourceLogsController.loading}
					<StatusBadge tone="info" dot pulse>刷新中</StatusBadge>
				{:else}
					<StatusBadge tone="success" dot>实时</StatusBadge>
				{/if}

				<!-- 暂停必须说清「你看到的是哪个时刻的数据」，否则暂停会退化成无声的数据过期 -->
				{#if sourceLogsController.paused && sourceLogsController.lastUpdatedAt}
					<span class="tabular text-xs whitespace-nowrap text-muted-foreground">
						数据截至 {formatClockTime(sourceLogsController.lastUpdatedAt)}
					</span>
				{/if}
			</div>

			<div class="flex items-center gap-2">
				<!--
					分段控件的外高必须与右侧 sm 档按钮一致：移动端 44px、md 以上 36px。
					做法是内层保持按钮级高度、外层只在 md 以上留 2px 内边距——
					移动端若同样留内边距，外框会涨到 48px 比按钮高 4px；
					反过来压缩内层又会把触控区压到 44px 以下，违背触屏工效下限。
				-->
				<div class="flex rounded-lg bg-muted px-0.5 md:p-0.5" role="group" aria-label="日志筛选">
					{#each views as item (item.value)}
						{@const active = sourceLogsController.view === item.value}
						<button
							type="button"
							aria-pressed={active}
							class={cn(
								'min-h-11 cursor-pointer rounded-md px-3.5 text-xs font-medium transition-colors duration-150 ease-out-soft md:min-h-8',
								active
									? 'bg-background text-foreground shadow-soft'
									: 'text-muted-foreground hover:text-foreground'
							)}
							onclick={() => sourceLogsController.setView(item.value)}
						>
							{item.label}
						</button>
					{/each}
				</div>

				<!--
					「暂停」是长驻页面的必要能力：日志每 5 秒整表替换，而「我想停下来看清一行」
					是高频需求，此前只能靠切走标签页（隐藏才会停）。
					手动「刷新」不受暂停影响——用户按了就该拉一次。
				-->
				<Button
					variant="ghost"
					size="sm"
					onclick={() => sourceLogsController.setPaused(!sourceLogsController.paused)}
				>
					{sourceLogsController.paused ? '继续' : '暂停'}
				</Button>

				<Button
					variant="ghost"
					size="sm"
					pending={sourceLogsController.loading}
					pendingLabel="刷新中…"
					onclick={() => sourceLogsController.refresh()}
				>
					刷新
				</Button>
			</div>
		</div>
	</CardHeader>

	<CardContent class="p-0">
		<!--
			失败降级：已有数据时只在表格上方加一条横幅，不接管整个面板。

			此前 `{#if error}` 的优先级高于表格，一次瞬时失败就会把 20 行日志连同
			滚动位置一起换成一个错误面板，而数据其实还在控制器里——用户正在看的
			内容被后台的一次抖动销毁了。这与音源列表页早已确立的原则一致
			（有数据时错误降级为横幅，见 sources/+page.svelte），此前只有日志没沿用。

			横幅刻意标注「以下为哪个时刻的数据」：降级之后用户仍需要知道
			自己看的是不是最新的一批，否则「保留数据」会变成「显示过期数据而不自知」。
		-->
		{#if sourceLogsController.error && sourceLogsController.logs.length > 0}
			<div
				class="flex flex-wrap items-start justify-between gap-3 border-b border-danger/20 bg-danger-soft/60 px-4 py-3"
			>
				<p class="text-xs leading-relaxed text-danger">
					日志刷新失败：{sourceLogsController.error}。{sourceLogsController.lastUpdatedAt
						? `以下为 ${formatClockTime(sourceLogsController.lastUpdatedAt)} 的数据。`
						: '以下为最近一次成功获取的数据。'}
				</p>
				<Button variant="ghost" size="sm" onclick={() => sourceLogsController.refresh()}>
					重试
				</Button>
			</div>
		{/if}

		{#if sourceLogsController.logs.length > 0}
			<!-- 列宽的两种档位及其来龙去脉见 hasSongMeta；下面各列只在有元数据档取宽度。 -->

			<!-- 桌面：表格承载多维对比，列宽两档都稳定，以便纵向扫读 -->
			<div class="hidden md:block">
				<!--
					表头必须吸顶。实测这 20 行日志的滚动内容高 1082px（行高 41~61px），
					而容器可视区只有 630px（max-h-[70vh]，900px 视口下）：不吸顶的话往下
					滚十来行，表头整行就离屏了（刻意不在这里列名单——列会增减，而论证不依赖
					有哪几列），而列里大量是无自解释性的值（2ms、flac24bit、05-19 14:50:33），
					用户只能靠位置猜列义。给容器一个高度上限后表头才吸得住，
					顺带把「滚完 1082px 才能看到最新那行」变成「容器内滚动」。
					滚动容器放在 Card 的 overflow-hidden 之内，无需改动卡片的圆角裁剪。
				-->
				<div class="max-h-[70vh] overflow-y-auto">
					<!--
						两种档位的分工见 hasSongMeta。w-full 是共有的：表在任何一档都要铺满卡片。
						aria-labelledby 让它有一个可读的名字——非歌曲列全是无自解释性的短值
						（2ms / flac24bit / 05-19 14:50:33），屏幕阅读器按表导航时需要先
						听到这张表是什么。视觉上的名字由相邻的 <h2> 承担，这里只是把它接上。
					-->
					<table
						class={cn('w-full text-left text-xs', hasSongMeta && 'table-fixed')}
						aria-labelledby="runtime-logs-heading"
					>
						<thead class="sticky top-0 z-10 bg-card">
							<tr class="text-muted-foreground">
								<!-- 两种档位都不定宽：有元数据时它吃剩余空间，无元数据时随内容比例摊。 -->
								<th scope="col" class={TH}>歌曲信息</th>
								<th scope="col" class={cn(hasSongMeta && 'w-28', TH)}>音质</th>
								<!--
									「错误」视图下每一行都是失败，状态列的 ERR 是零信息，
									整列让出去（见 showOutcome）。
								-->
								{#if showOutcome}
									<th scope="col" class={cn(hasSongMeta && 'w-20', TH)}>状态</th>
								{/if}
								<!--
									按最宽取值定档：`2/0`（3 个等宽字符）与两字表头在 w-20(80px) 下
									都放得下，与同为短值的「状态」「延迟」同档，不另立新档。
								-->
								{#if hasUpstreamStatus}
									<th scope="col" class={cn(hasSongMeta && 'w-20', TH)}>上游</th>
								{/if}
								<!--
									时间列按最宽形态取档：跨天时是 `MM-DD HH:mm:ss` 共 14 字符，
									12px 下约 94px，加上左右 px-4 需要 126px，故取 w-36(144px)。
									此前按当天的 8 字符反推出 w-24(96px)，跨天行会溢出到延迟列。
								-->
								<th scope="col" class={cn(hasSongMeta && 'w-36', TH)}>时间</th>
								<th scope="col" class={cn(hasSongMeta && 'w-20', TH)}>延迟</th>
								<!--
									诊断列只在有元数据档取固定档，且刻意做成响应式（md 更窄、
									lg 才放宽）：那时它退居次要，宽度吃紧时应当由它先让位，而不
									是挤压歌名。lg 取 w-64(256px) 曾被实测推出来（首行最宽的
									「刷新登录 + 歌曲 hash + 已刷新 Token」需要 208px）。移走
									歌曲 hash 后所需宽度下降约 60px，这个档位从「刚好够」变成
									「有余量」，因此无需重新定档；md 档同理。
								-->
								<th scope="col" class={cn(hasSongMeta && 'w-40 lg:w-64', TH)}>诊断</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-border/70">
							{#each sourceLogsController.logs as log (log.log_id)}
								{@const isNew = sourceLogsController.newLogIds.has(log.log_id)}
								<tr
									class="transition-colors hover:bg-muted/40"
									style={rowFlashStyle(isNew)}
								>
									<td class="min-w-0 px-4 py-2.5">
										<div class="flex flex-col gap-0.5">{@render songCell(log)}</div>
									</td>
									<td class="px-4 py-2.5">{@render qualityCell(log)}</td>
									{#if showOutcome}
										<td class="px-4 py-2.5">{@render outcomeBadge(log)}</td>
									{/if}
									{#if hasUpstreamStatus}
										<td class="px-4 py-2.5">{@render upstreamCell(log)}</td>
									{/if}
									<td
										class="tabular px-4 py-2.5 whitespace-nowrap text-muted-foreground"
										title={formatDateTime(log.created_at)}
									>
										{formatLogTime(log.created_at)}
									</td>
									<td
										class="tabular px-4 py-2.5 whitespace-nowrap font-mono"
										title={latencyTitle(log)}
									>
										{latencyText(log)}
									</td>
									<td class="px-4 py-2.5">
										<div class="flex flex-col items-start gap-1">
											{@render diagMeta(log)}
											{#if !log.ok && (log.error_code || log.error)}
												{@render failureBadge(log)}
											{/if}
										</div>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</div>

			<!-- 移动端：改为分行卡片，避免最小宽度逼出横向滚动条 -->
			<ul class="divide-y divide-border/70 md:hidden">
				{#each sourceLogsController.logs as log (log.log_id)}
					{@const isNew = sourceLogsController.newLogIds.has(log.log_id)}
					<li class="space-y-2 px-4 py-3" style={rowFlashStyle(isNew)}>
						<div class="flex items-start justify-between gap-3">
							<div class="min-w-0 flex-1 text-xs">{@render songCell(log)}</div>
							{#if showOutcome}
								{@render outcomeBadge(log)}
							{/if}
						</div>

						<!-- 与桌面表格同一个失败主表达：中文标签在窄屏下同样不需要折行 -->
						{#if !log.ok && (log.error_code || log.error)}
							{@render failureBadge(log)}
						{/if}

						<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5">
							{@render qualityCell(log)}
							<!--
								上游观测在窄屏下必须一起出来，否则这一列会静默变成桌面端专属
								（窄屏用的是卡片渲染，不读桌面那张表的列）。

								两点与桌面不同，都是窄屏没有表头逼出来的：

								① 把列名「上游」折叠进值里。桌面端靠表头说明这一格是什么，
								   这里没有表头，光一个 `2/0` 是纯噪声。
								② 没有观测就整块不渲染，而不是像桌面那样显示 `—`：窄屏这一行
								   是若干无标签的短值，多一个孤零零的 `—` 只会让人猜它在说什么
								   （桌面端的 `—` 有列头兜住语义，成立）。
							-->
							{#if hasUpstreamStatus && log.upstream_status !== null}
								<span class="text-caption text-muted-foreground" title={upstreamTitle(log)}>
									上游 {upstreamText(log)}
								</span>
							{/if}
							<span
								class="tabular text-caption text-muted-foreground"
								title={formatDateTime(log.created_at)}
							>
								{formatLogTime(log.created_at)}
							</span>
							<span
								class="tabular font-mono text-caption text-muted-foreground"
								title={latencyTitle(log)}
							>
								{latencyText(log)}
							</span>
						</div>

						<!-- 与桌面表格保持信息等价：阶段（歌曲 hash 已随 songCell 上移）、
						     降级 / 刷新 / 重试标记。错误码与原因由上面的 failureBadge 承担，
						     上游观测的两个数字在上面的元数据行里，桌面端同样如此。 -->
						{@render diagMeta(log)}
					</li>
				{/each}
			</ul>
		{:else if sourceLogsController.error}
			<!-- 零行 + 错误：此时没有可保留的内容，才让错误接管整个面板 -->
			<div class="p-8 text-center">
				<p class="text-sm leading-relaxed text-danger">
					加载日志失败：{sourceLogsController.error}
				</p>
				<Button size="sm" class="mt-4" onclick={() => sourceLogsController.refresh()}>重试</Button>
			</div>
		{:else if sourceLogsController.loading}
			<div class="space-y-2 p-4">
				{#each skeletonSlots as slot (slot)}
					<Skeleton class="h-9 w-full" />
				{/each}
			</div>
		{:else}
			<div class="px-6 py-14 text-center">
				<p class="text-sm text-muted-foreground">
					{sourceLogsController.view === 'errors'
						? '暂无错误记录，一切正常。'
						: '暂无运行时流量记录，播放音乐后这里会出现取链日志。'}
				</p>
			</div>
		{/if}
	</CardContent>
</Card>

# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- 音源标题可自定义：新增 `PATCH /api/v1/admin/sources/{source_id}`，传入 `name` 改名，传 `null` 或纯空白恢复默认名；音源列表与详情新增 `name`、`default_name` 字段。
- 音源详情回显官方建议长度（标题 24 字符、描述 36 字符），仅供后台计数提示，不作硬校验、不参与拒绝逻辑。
- 标题在写入与脚本渲染双侧清洗 `*/` 与控制字符，避免自定义标题提前闭合脚本注释导致导入即语法错误；写入路径另有 256 字节技术护栏。
- 运行时日志落盘 `upstream_quality`：记录该行所述那次上游调用**实际**使用的酷狗质量参数（`128` / `320` / `flac` / `high`），供界面回显，不必再在前端复制一张映射表。只有描述 `song/url` 取链的那一行才有值——`ensure_dfid` / `refresh_login` 两次调用不带质量参数，给它们写一个「本来会要的质量」等于发明一件没发生过的事。字段带 `#[serde(default)]`，本次改动之前落盘的历史 jsonl 行仍能整行读入，读出为 `null`（语义是「这一行没有记录」，不是「这一行没有音质」）。

### Changed

- `PUBLIC_BASE_URL` / `KUGOU_API_BASE_URL` 现在会在启动时校验：必须是带 `http`/`https` scheme 的绝对地址，否则直接拒绝启动并给出具体原因。只写 `localhost:8787`（漏 scheme）这类配置此前会静默生成不可用的脚本地址，现在会在本机明确报错。配置值同时被归一化（去尾随 `/`、host 转小写），`PUBLIC_BASE_URL=http://host/` 不再导致脚本 URL 出现双斜杠。
- 脚本中插入的地址、token 与平台名改用 JSON 字符串字面量生成，`PUBLIC_BASE_URL` 含单引号等字符时不再破坏脚本语法。
- 脚本头部元信息按语义分工：`@name` 为可自定义的实例标题，`@description` 由服务端独占写入「服务地址 · 账号 ID」，便于一眼区分本地与生产实例；免责声明下沉到脚本体注释。
- LX 音源列表中的平台显示名固定为「酷狗概念版」，不再跟随用户自定义标题。
- 脚本 `@version` 与后端发布版本刻意解耦，并补充三档 bump 规则；本轮脚本模板变化按规则由 `0.1.1` 升至 `0.1.2`。

## [0.1.3] - 2026-09-01

### Changed

- 临时将预构建 Docker 镜像限制为 `linux/amd64`，待构建链路修复后恢复多架构支持。
- 调整镜像发布顺序，先构建 KuGouMusicApi，避免发布不完整的版本。

### Fixed

- 修复 KuGouMusicApi Docker 构建因 pnpm 忽略 `esbuild` build script 而失败的问题。

## [0.1.2] - 2026-08-31

### Added

- 发布支持 `linux/amd64` 和 `linux/arm64` 的预构建 Docker 镜像。
- 增加 GitHub Actions，自动发布 `kg2lx` 和 `KuGouMusicApi` 镜像。

### Changed

- 优化 Docker 镜像构建缓存。
- Docker Compose 部署文档改为使用预构建镜像。

### Fixed

- 修复移动端音源请求 body 双重序列化导致的播放失败。
- 后端兼容旧版脚本发送的 JSON 字符串包装请求体。

[Unreleased]: https://github.com/DreamAlone666/kg2lx/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/DreamAlone666/kg2lx/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/DreamAlone666/kg2lx/compare/v0.1.1...v0.1.2

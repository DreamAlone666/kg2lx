# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed

- 音源列表卡片改为展示**账号状态**与**最近刷新时间**。此前列表只有 VIP 标记，且时间取自源记录的 `updated_at`（刷新不会改动它），导致点击刷新后卡片上的数值纹丝不动；现在展示的是账号侧 `last_refresh_at`，与详情页同源。
- 全站时间统一为定长 24 小时制 `YYYY-MM-DD HH:mm:ss`，消除同一屏内 12/24 小时制混用，以及窄信息格里的折行。
- 运行时日志表格的列宽改为按内容类型定档、主内容列吃剩余空间。此前 800px 视口实测歌曲信息列只剩 126px 而诊断列独占 256px，宽度排序与重要性排序相反。
- 运行时日志的失败信息收敛为单次表达（错误码与错误原因同行，装不下时整段落到第二行），不再由状态列、诊断列重复表达三遍。
- 异步动作的反馈就近到动作发生的位置：`Button` 新增 `pending` / `pendingLabel`，统一「切换文案 + 置 disabled + 标记 `aria-busy`」这一此前被手写多处的模式；音源列表在刷新按钮旁就地给出「列表更新于 HH:mm:ss」。
- 运行时日志的轮询失败不再清空列表：已有数据时降级为表格上方的横幅，并标明「以下为 HH:mm:ss 的数据」；只有零行时才由错误接管整个面板。
- 运行时日志的状态徽章纳入失败态：轮询静默失败时不再显示绿色「实时」，而是「刷新失败」。

### Fixed

- 修复 `AccountStatus::LoginFailed` 被序列化为 `loginfailed` 的问题：前端词典查的是 `login_failed`，「登录失败」因此长期回退成原始英文串。
- 移除音源卡片与详情页中重复且均被截断的 `source_id` 展示——内部主键，地址栏里就是同一个值。
- 「上一次刷新尚未结束」不再被提示成「刷新失败，请稍后重试」：`refreshSource` 改为返回 `ok` / `failed` / `busy` 三态，调用方不再为一件没有发生的失败责备用户。

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

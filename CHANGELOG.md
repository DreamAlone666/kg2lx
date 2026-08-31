# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

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

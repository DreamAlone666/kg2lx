# kg2lx 管理后台

SvelteKit 管理后台提供酷狗概念版扫码登录、账号与会员权益管理、洛雪脚本接入和全站事件日志。账号列表及详情位于 `/accounts`，全站日志位于 `/logs`。

在账号详情页可以同步快照、开启按需自动领取 VIP、手动领取或核对当日记录。全站日志可按账号和事件类型筛选，并查看同一请求的事件链。

## 本地开发

启动 Rust 后端后，在本目录执行以下命令。

```sh
pnpm install --frozen-lockfile
pnpm dev
```

连接页可以将后端地址留空。Vite 将 `/healthz` 和 `/api/*` 原样代理给 Rust 后端，默认目标为 `http://127.0.0.1:8787`。后端监听其他端口时可指定代理目标。

```sh
VITE_BACKEND_TARGET=http://127.0.0.1:9000 pnpm dev
```

## 检查与部署

```sh
pnpm check
pnpm build
```

构建产物在 `build/`。将服务端的 `WEB_DIST_DIR` 指向该目录即可同源托管。部署后在连接页输入 `ADMIN_TOKEN`，后端地址留空。

旧 `/sources` 页面已由账号列表和详情页接替。现有洛雪脚本升级及后端接口、数据卷变化见服务端说明。 [升级适配](../server/README.md#升级适配)

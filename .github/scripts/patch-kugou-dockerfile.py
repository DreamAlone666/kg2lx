#!/usr/bin/env python3
"""给 KuGouMusicApi 的 Dockerfile 打上那处临时补丁。

上游在 node:lts-alpine 里跑 pnpm install 时，pnpm 会跳过 esbuild 的构建脚本；这处补丁
让本次安装显式允许 esbuild 执行构建脚本，装完再把临时文件删掉。

用法：patch-kugou-dockerfile.py [Dockerfile 路径]，默认 KuGouMusicApi/Dockerfile。

为什么单独成文件、而不是内联在 workflow 里：上游镜像的 tag 必须同时覆盖「上游提交」
与「这处补丁」两者（见 publish-images.yml 的 Check upstream image 一步），而补丁签名取自
本文件的哈希。内联就只能对整个 workflow 取哈希，那样改一句发行说明也会让上游镜像无谓
重建。签名的另一面是：本文件一改、tag 必变、镜像必重建，不会出现「补丁改了但镜像被判定
已存在而跳过构建」这种全绿却产物陈旧的情形。
"""
import sys
from pathlib import Path

DEFAULT_TARGET = "KuGouMusicApi/Dockerfile"

OLD = "RUN pnpm install --prod --frozen-lockfile"
NEW = (
    "RUN printf '%s\\n' 'allowBuilds:' '  esbuild: true' > pnpm-workspace.yaml \\\n"
    "    && pnpm install --prod --frozen-lockfile \\\n"
    "    && rm pnpm-workspace.yaml"
)


def main(argv: list[str]) -> int:
    target = Path(argv[1] if len(argv) > 1 else DEFAULT_TARGET)
    text = target.read_text(encoding="utf-8")
    if OLD not in text:
        raise SystemExit(
            f"补丁目标不在 {target} 里：{OLD!r}。上游 Dockerfile 变了，请同步更新本脚本——"
            f"少打这处补丁的镜像会照样构建成功，问题要到运行时才现形。"
        )
    target.write_text(text.replace(OLD, NEW, 1), encoding="utf-8")
    print(f"已给 {target} 打上 esbuild allowBuilds 补丁")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

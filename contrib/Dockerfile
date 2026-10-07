# syntax=docker/dockerfile:1

# 构建阶段。刻意与运行阶段分离：rust 镜像 ~1.3GB，运行镜像 ~120MB。
FROM docker.io/library/rust:1-slim-bookworm AS builder

WORKDIR /build

# zstd / cc 这类 crate 需要 C 编译器；libgit2、cmake、libssl 不再需要
# （git 操作已改为调用 git 命令行，git2 依赖已移除）。
RUN apt-get update && apt-get install -y --no-install-recommends \
        gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*

# 依赖单独成层：改源码时这一层命中缓存，不用重新编译 200+ 个 crate。
# 先用占位源码把依赖图解出来，删掉产物，再拷真实源码。
COPY Cargo.toml Cargo.lock ./
COPY gitlook-server/Cargo.toml ./gitlook-server/
COPY gitlook-cli/Cargo.toml ./gitlook-cli/

RUN mkdir -p gitlook-server/src gitlook-cli/src \
    && echo "" > gitlook-server/src/lib.rs \
    && echo "fn main() {}" > gitlook-server/src/main.rs \
    && echo "fn main() {}" > gitlook-cli/src/main.rs \
    && cargo build --release \
    && rm -rf gitlook-server/src gitlook-cli/src

# 真实源码。
# 必须 touch：COPY 会保留宿主机文件的原始 mtime，而真实源码通常比上一层的
# 占位产物还旧，cargo 的 fingerprint 只看 mtime，会判定"没变化"而跳过编译，
# 结果 COPY 进去的仍是占位二进制（两个文件大小完全相同就是征兆）。
COPY gitlook-server ./gitlook-server
COPY gitlook-cli ./gitlook-cli
RUN set -e; \
    find gitlook-server/src gitlook-cli/src -name '*.rs' -exec touch {} +; \
    cargo build --release; \
    s=$(stat -c%s target/release/gitlook-server); \
    c=$(stat -c%s target/release/gitlook-cli); \
    echo "built: server=$s cli=$c"; \
    if [ "$s" -lt 5000000 ] || [ "$c" -lt 1000000 ] || [ "$s" -eq "$c" ]; then \
        echo "产物大小异常，多半是占位二进制没被替换"; \
        exit 1; \
    fi

# 运行阶段
FROM docker.io/library/debian:bookworm-slim

# git 是运行期硬依赖：post-receive 钩子靠它把推送内容 checkout 到 worktree。
RUN apt-get update && apt-get install -y --no-install-recommends \
        git ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/gitlook-server /usr/local/bin/
COPY --from=builder /build/target/release/gitlook-cli /usr/local/bin/

# 容器内以 root 运行，这是刻意的：
# rootless podman 下容器 uid 0 映射到宿主机当前用户，这样 bind mount 进来的
# 仓库目录在容器里看到的属主与 git 的 euid 一致，不会触发
# "detected dubious ownership"。若改成非 root（如 uid 1000），会映射到
# subuid，git 会拒绝操作宿主机目录。
#
# 刻意不设 USER，因此没有任何写保护；配置、模板、HowTo 全部通过
# compose 的 bind mount 挂进来，宿主机上改完立即生效。
WORKDIR /var/lib/gitlook

EXPOSE 9999

ENTRYPOINT ["/usr/local/bin/gitlook-server"]
CMD ["--config", "/etc/gitlook/config.toml"]
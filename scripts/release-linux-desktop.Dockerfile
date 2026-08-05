# syntax=docker/dockerfile:1.7
FROM node:22-bookworm AS build

ARG TARGETARCH
ARG RUST_VERSION=1.88.0

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential \
    clang \
    cmake \
    curl \
    file \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libxdo-dev \
    patchelf \
    wget \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 --retry 5 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" --no-modify-path
ENV PATH="/root/.cargo/bin:${PATH}"
ENV CARGO_TARGET_DIR=/workspace/target

WORKDIR /workspace
COPY . .

RUN cd apps/desktop && npm ci && npm run build
RUN cargo build --locked --profile dist -p awm-desktop
RUN cd apps/desktop && npm run tauri -- build --ci \
    --config '{"build":{"beforeBuildCommand":""}}' \
    --runner /workspace/scripts/tauri-dist-runner.sh \
    --bundles appimage,deb

RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) LABEL=x86_64 ;; \
      arm64) LABEL=arm64 ;; \
      *) echo "unsupported Docker architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    APPIMAGE="$(find target/release/bundle/appimage -maxdepth 1 -type f -name '*.AppImage' -print -quit)"; \
    DEB="$(find target/release/bundle/deb -type f -name '*.deb' -print -quit)"; \
    test -n "$APPIMAGE"; \
    test -n "$DEB"; \
    mkdir -p /artifacts; \
    install -m 755 "$APPIMAGE" "/artifacts/awm-desktop-linux-${LABEL}.AppImage"; \
    install -m 644 "$DEB" "/artifacts/awm-desktop-linux-${LABEL}.deb"

FROM scratch AS artifacts
COPY --from=build /artifacts/ /

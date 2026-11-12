FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential debhelper dpkg-dev lintian python3 pkg-config libasound2-dev \
    rustc-1.97 cargo-1.97 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /opt/ppa-toolchain /work \
    && ln -s /usr/bin/cargo-1.97 /opt/ppa-toolchain/cargo \
    && ln -s /usr/bin/rustc-1.97 /opt/ppa-toolchain/rustc \
    && ln -s /usr/bin/rustdoc-1.97 /opt/ppa-toolchain/rustdoc \
    && useradd --create-home --uid 1001 builder \
    && chown builder:builder /work
ENV PATH="/opt/ppa-toolchain:${PATH}"
ENV TERM=xterm-256color LC_ALL=C.UTF-8
USER builder
WORKDIR /work

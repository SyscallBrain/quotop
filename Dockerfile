# syntax=docker/dockerfile:1
#
# quotop in a container: a static binary on an empty (`scratch`) image.
#
#   docker build -t quotop .                 # build from source
#   docker run --rm -it quotop               # see README.md → Docker
#
# Two final targets share the same layout:
#   - `source` (the default) compiles quotop from this directory;
#   - `prebuilt` packages binaries built elsewhere (the release workflow puts
#     them in `dist/<arch>/quotop`), so the multi-arch image doesn't have to
#     compile under emulation.

ARG RUST_VERSION=1
ARG ALPINE_VERSION=3

# The filesystem the program needs: a home any user can write to (so the image
# works with `--user "$(id -u):$(id -g)"`), /tmp, and time zone data for `TZ`.
FROM alpine:${ALPINE_VERSION} AS rootfs
RUN apk add --no-cache tzdata \
    && mkdir -p /rootfs/home/quotop /rootfs/tmp /rootfs/usr/share \
    && chmod 1777 /rootfs/home/quotop /rootfs/tmp \
    && cp -r /usr/share/zoneinfo /rootfs/usr/share/zoneinfo

FROM rust:${RUST_VERSION}-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY locales ./locales
RUN cargo build --release --locked && cp target/release/quotop /quotop

FROM scratch AS prebuilt
ARG TARGETARCH
COPY --from=rootfs /rootfs/ /
COPY --chmod=0755 dist/${TARGETARCH}/quotop /quotop
ENV HOME=/home/quotop
USER 65532:65532
ENTRYPOINT ["/quotop"]
LABEL org.opencontainers.image.title="quotop" \
      org.opencontainers.image.description="A terminal dashboard for the balances, credits and quotas of your API services" \
      org.opencontainers.image.source="https://github.com/SyscallBrain/quotop" \
      org.opencontainers.image.licenses="MIT"

FROM scratch AS source
COPY --from=rootfs /rootfs/ /
COPY --from=build /quotop /quotop
ENV HOME=/home/quotop
USER 65532:65532
ENTRYPOINT ["/quotop"]
LABEL org.opencontainers.image.title="quotop" \
      org.opencontainers.image.description="A terminal dashboard for the balances, credits and quotas of your API services" \
      org.opencontainers.image.source="https://github.com/SyscallBrain/quotop" \
      org.opencontainers.image.licenses="MIT"

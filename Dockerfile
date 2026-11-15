# The foldz image: the binary with file capabilities for its RT thread,
# libmxl from the samplez builder it is built and tested with, and the
# licences of everything shipped.
FROM ghcr.io/danielclasen/samplez-builder@sha256:92ece4452be98fff177b49fb201c8c693130c08845c42725dd5b1173d4863fba AS build

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

ARG CARGO_ABOUT_VERSION=0.9.3
# Caps cargo's parallel jobs when set.
ARG CARGO_BUILD_JOBS

RUN apt-get update \
 && apt-get install -y --no-install-recommends libcap2-bin \
 && rm -rf /var/lib/apt/lists/*

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install --locked --root /opt/cargo-about --features cli \
      ${CARGO_BUILD_JOBS:+--jobs "$CARGO_BUILD_JOBS"} \
      "cargo-about@${CARGO_ABOUT_VERSION}" \
 && /opt/cargo-about/bin/cargo-about --version

WORKDIR /src
COPY . .
# cap_sys_nice and cap_ipc_lock are permitted, not effective: exec never
# fails without them, and samplez raises what the container grants
# (`capabilities.add`) into its effective set at start.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin foldz \
      ${CARGO_BUILD_JOBS:+--jobs "$CARGO_BUILD_JOBS"} \
 && install -Dm0755 target/release/foldz /out/usr/local/bin/foldz \
 && setcap cap_sys_nice,cap_ipc_lock=p /out/usr/local/bin/foldz \
 && getcap /out/usr/local/bin/foldz \
 && install -Dm0644 LICENSE /out/usr/share/licenses/foldz/LICENSE \
 && /opt/cargo-about/bin/cargo-about generate --locked --fail \
      -o /out/usr/share/licenses/foldz/THIRD-PARTY.txt about.hbs

# The binary and the licences of the crates in it, attached to a release.
FROM scratch AS dist
COPY --from=build /out/usr/local/bin/foldz /out/usr/share/licenses/foldz/THIRD-PARTY.txt /

FROM debian:trixie-20261027@sha256:2dc419c6d7fe3dad0a2f9fd469c34cc90b0f9ba642d8fde837fe3d7d34a0e7f1

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

# libmxl is a C++ library; the base image ships libstdc++6 for apt.
COPY --from=build /opt/mxl/lib/libmxl.so.1.1 /opt/mxl/lib/libmxl.so.1.1
RUN ln -s libmxl.so.1.1 /opt/mxl/lib/libmxl.so.1 \
 && ln -s libmxl.so.1 /opt/mxl/lib/libmxl.so
COPY --from=build /opt/mxl/share/licenses /opt/mxl/share/licenses
COPY --from=build /out/ /

# 65532 owns nothing in the image; a deployment runs it as the uid that
# owns the MXL domain.
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/foldz"]
CMD ["--config", "/etc/foldz/config.json"]

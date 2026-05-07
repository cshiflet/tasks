# One-shot init container that creates the Etebase test user
# (default `alice / alicepw`) once the etebase service is up.
# Idempotent — re-runs are no-ops once the user exists.
#
# The `etebase` PyPI package (last released 2021) ships wheels
# only up to cp310, so any current Python forces pip through a
# source build that needs the Rust toolchain + libsodium-dev.
# Two-stage so the runtime image doesn't carry rustup or apt
# build deps — final image is python:3.11-slim + libsodium23 +
# the installed package tree.

# -------- build stage --------
FROM python:3.11-slim AS build

# build-essential pulls gcc + the linker; pkg-config + libsodium-dev
# satisfy the etebase Rust crate's link-time deps; curl bootstraps
# rustup; ca-certificates is needed for the rustup HTTPS fetch.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates libsodium-dev \
    && rm -rf /var/lib/apt/lists/*

# Minimal rustup — no docs, no test runners, no cross targets.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_NO_CACHE_DIR=1

# `--target=/install` deposits a self-contained site-packages tree
# we can COPY verbatim into the runtime stage. Avoids the `--user`
# vs. system-pip vs. uid-mismatch headaches that --user causes when
# we drop privileges below.
RUN pip install --target=/install 'etebase>=0.30,<1'

# -------- runtime stage --------
FROM python:3.11-slim

# libsodium runtime symbols only; no -dev, no compiler.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libsodium23 \
    && rm -rf /var/lib/apt/lists/*

# Drop the etebase + msgpack package trees into Python's system
# site-packages so they're importable without PYTHONPATH gymnastics.
COPY --from=build /install /usr/local/lib/python3.11/site-packages

COPY bootstrap.py /usr/local/bin/etebase-bootstrap.py
RUN chmod +x /usr/local/bin/etebase-bootstrap.py

RUN useradd --system --uid 9999 --no-create-home --gecos '' etebot
USER etebot:etebot

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

ENTRYPOINT ["python", "/usr/local/bin/etebase-bootstrap.py"]

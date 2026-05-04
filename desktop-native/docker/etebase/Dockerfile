# Etebase test-server image, built from upstream source.
#
# `etesync/server` doesn't publish a Docker Hub image — the Docker
# Hub repo of that name is squatted / 404 — so we clone the
# project at build time and run it under Django's dev server. The
# resulting container is for local iteration only; do not expose
# it to the public internet.

FROM python:3.11-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        git \
        curl \
        build-essential \
        libssl-dev \
        libffi-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Pin the upstream ref via a build-arg so a regression in master
# doesn't silently break the test stack. Override with
# `--build-arg ETEBASE_REF=v0.13.0` if you want a specific tag.
ARG ETEBASE_REF=master
RUN git clone --depth 1 --branch ${ETEBASE_REF} \
        https://github.com/etesync/server.git . && \
    pip install --no-cache-dir -r requirements.txt

EXPOSE 3735

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]

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

# Pin upstream to a known-good ref. `master` has historically
# broken the WSGI app path (we hit
# `ModuleNotFoundError: No module named 'etebase_server.wsgi'`
# on May 2026 master). v0.13.0 is the most recent tag verified
# end-to-end with `manage.py runserver`. Override with
# `--build-arg ETEBASE_REF=...` if you want a specific tag.
ARG ETEBASE_REF=v0.13.0
RUN git clone --depth 1 --branch ${ETEBASE_REF} \
        https://github.com/etesync/server.git . && \
    pip install --no-cache-dir -r requirements.txt && \
    # daphne fallback in case a future tag drops `wsgi.py` and
    # the entrypoint needs to switch to the ASGI app.
    pip install --no-cache-dir daphne

EXPOSE 3735

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]

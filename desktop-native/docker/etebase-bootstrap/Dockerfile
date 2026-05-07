# One-shot init container that creates the Etebase test user
# (default `alice / alicepw`) once the etebase service is up.
# Idempotent — re-runs are no-ops once the user exists.
#
# Pin Python 3.11. The `etebase` PyPI package is a maturin-built
# wheel last released in 2021 with cp37–cp311 binaries; Python
# 3.12 forces a source build that needs the Rust toolchain +
# libsodium headers, which we'd rather not drag into a 50 MB
# bootstrap image. 3.11-slim has glibc (vs musl on Alpine) so
# the wheel installs cleanly without compatibility tags drama.
FROM python:3.11-slim

# Quieter pip output + no .pyc droppings in the layer.
ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_NO_CACHE_DIR=1

RUN pip install --no-cache-dir 'etebase>=0.30,<1'

COPY bootstrap.py /usr/local/bin/etebase-bootstrap.py
RUN chmod +x /usr/local/bin/etebase-bootstrap.py

# Run as non-root once we've installed everything.
RUN useradd --system --uid 9999 --no-create-home --gecos '' etebot
USER etebot:etebot

ENTRYPOINT ["python", "/usr/local/bin/etebase-bootstrap.py"]

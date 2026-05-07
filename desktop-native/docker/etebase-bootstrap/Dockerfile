# One-shot init container that creates the Etebase test user
# (default `alice / alicepw`) once the etebase service is up.
# Idempotent — re-runs are no-ops once the user exists.
#
# We pin a thin Python base so the etebase wheel install is fast
# and the resulting image stays small. python:3.12-slim ships
# glibc, which avoids the musl-wheel-compatibility hazard the
# Alpine variant has for native-extension wheels.
FROM python:3.12-slim

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

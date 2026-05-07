#!/usr/bin/env python3
"""
Idempotent Etebase test-user bootstrap.

Run as a one-shot sidecar after the Etebase server reports
healthy. Tries Account.login first; if that succeeds the user
already exists with the right password and we exit happy.
Otherwise tries Account.signup against the public endpoint
(requires AUTO_SIGNUP=true on the server) and exits 0 on
success. The desktop client doesn't need this script to work —
its signup-fallback path does the same thing on first sign-in.
This is for the CI / pre-warmed-stack workflow where you want
the user ready before any client connects.

Reads server / user / email / password from env vars set in
docker-compose.test-servers.yml. Bash-safe defaults make the
script runnable standalone for debugging too.
"""
import os
import sys
import time

import etebase
from etebase import HttpException


def env(name: str, default: str) -> str:
    v = os.environ.get(name, default).strip()
    return v or default


SERVER_URL = env("ETEBASE_SERVER_URL", "http://etebase:3735")
USERNAME = env("ETEBASE_TEST_USER", "alice")
EMAIL = env("ETEBASE_TEST_EMAIL", "alice@example.com")
PASSWORD = env("ETEBASE_TEST_PASSWORD", "alicepw")
APP_NAME = "tasks-bootstrap"


def main() -> int:
    print(f"bootstrap: server={SERVER_URL} user={USERNAME}")

    # The healthcheck in compose covers most of this, but a fresh
    # container with REGEN_INI takes a beat after /healthz before
    # the auth blueprint is wired. Five tries at 1s spacing is
    # plenty.
    last_err = None
    for attempt in range(5):
        try:
            client = etebase.Client(APP_NAME, SERVER_URL)
            break
        except Exception as e:
            last_err = e
            time.sleep(1)
    else:
        print(f"bootstrap: couldn't reach Etebase at {SERVER_URL}: {last_err}",
              file=sys.stderr)
        return 1

    try:
        etebase.Account.login(client, USERNAME, PASSWORD)
        print(f"bootstrap: {USERNAME} already exists and login works — nothing to do")
        return 0
    except HttpException as e:
        print(f"bootstrap: login probe failed (status={e.status}); attempting signup")
    except Exception as e:
        # Network / TLS / unexpected — surface but try signup anyway,
        # since the network path may have flaked once.
        print(f"bootstrap: login probe surfaced {type(e).__name__}: {e}; "
              "attempting signup", file=sys.stderr)

    try:
        user = etebase.User(USERNAME, EMAIL)
        etebase.Account.signup(client, user, PASSWORD)
        print(f"bootstrap: signed up {USERNAME} (email={EMAIL})")
        return 0
    except HttpException as e:
        # Most likely AUTO_SIGNUP=false on the server, or the user
        # was created concurrently between our login probe and the
        # signup call (status 409). Treat the latter as success.
        if e.status == 409:
            print(f"bootstrap: {USERNAME} signed up concurrently — fine")
            return 0
        print(f"bootstrap: signup failed (status={e.status}): {e}",
              file=sys.stderr)
        return 1
    except Exception as e:
        print(f"bootstrap: signup raised {type(e).__name__}: {e}",
              file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

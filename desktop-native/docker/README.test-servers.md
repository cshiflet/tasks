# Local sync test servers

`docker-compose.test-servers.yml` brings up two local sync targets
the desktop client can talk to without registering anywhere or
hitting the public internet:

- **Etebase** — built locally from the vendored copy of
  [`victor-rds/docker-etebase`][upstream] under `./etebase/`.
  See [`etebase/UPSTREAM.md`](etebase/UPSTREAM.md) for the pinned
  upstream commit + bump procedure. No Docker Hub image is
  pulled; the first `up` builds the image.
- **Radicale** — `tomsquest/docker-radicale:latest` from Docker
  Hub.

[upstream]: https://github.com/victor-rds/docker-etebase

```sh
cd desktop-native/docker
docker compose -f docker-compose.test-servers.yml up -d --build
```

The `--build` flag is harmless on subsequent runs (Docker caches
the layers) but is required the first time so the local Etebase
image gets compiled. Bring everything down with:

```sh
docker compose -f docker-compose.test-servers.yml down       # keep volumes
docker compose -f docker-compose.test-servers.yml down -v    # wipe volumes
```

## Credentials

Default values, hard-coded in the compose file. **Intentionally
weak — never reuse on a publicly-reachable server.**

| Service  | Endpoint                       | Account       | Password                          |
|----------|--------------------------------|---------------|-----------------------------------|
| Etebase  | `http://127.0.0.1:3735`        | `alice`       | `alicepw`                         |
| Etebase  | `http://127.0.0.1:3735/admin/` | `admin` (Django superuser) | `changeme`                        |
| Radicale | `http://127.0.0.1:5232/test/`  | `test`        | `test`                            |
| Radicale | `http://127.0.0.1:5232/.web/`  | `test`        | `test`                            |

The Etebase service ships with `AUTO_SIGNUP=true`, so anonymous
clients can call the public signup endpoint. **`alice` is created
automatically** by the `etebase-bootstrap` sidecar service in the
compose file, which runs once after the Etebase server reports
healthy and uses the Etebase Python SDK to either log in or
sign up the test user. The script is idempotent — re-running
`up` after the user exists is a no-op.

The desktop client also has its own auto-signup fallback (gated
on loopback URLs) as a belt-and-braces second path, so even
without the bootstrap sidecar the first sign-in from the
desktop will create `alice`. The sidecar exists so CI / pre-warmed
stacks have the user ready before any client connects, and so
clients without a signup-fallback (e.g. mobile, third-party
Etebase apps) can connect to the test stack without ceremony.

> **Why a sidecar instead of `manage.py` shell?** Etebase's login
> is challenge-response over a per-user keypair; only the public
> key lives on the server, and it's uploaded by a real client
> during signup. A `manage.py shell` `User.objects.create_user(...)`
> creates a Django-auth row that can sign into `/admin/` but is
> invisible to the Etebase API. Bootstrap therefore needs a
> client SDK call, which is what the sidecar runs.

## Verify the stack is reachable

```sh
curl -sSI http://127.0.0.1:3735/ | head -1   # Etebase
curl -sSI http://127.0.0.1:5232/ | head -1   # Radicale
```

If either returns *connection refused* or hangs:

```sh
docker compose -f docker-compose.test-servers.yml ps
docker compose -f docker-compose.test-servers.yml logs etebase
docker compose -f docker-compose.test-servers.yml logs radicale
```

## Connecting the desktop client

### EteSync (Etebase)

1. Launch the client (`cargo run -p tasks-ui` from `desktop-native/`).
2. Open **Settings → Accounts → Add account**.
3. Fill in:
   - **Type**:       EteSync
   - **Label**:      `Local Etebase` (or anything)
   - **Server URL**: `http://127.0.0.1:3735`
   - **Username**:   `alice`
   - **Password**:   `alicepw`
4. Click **Add account**, then **Sync now** on the new row in
   the sidebar (or use the per-account sync button on the
   account header).

A successful sync flips the row state from `Idle` → `Syncing…` →
`Synced (N↓ / 0↑)`. The first sync against a fresh server creates
the `alice` user automatically (auto-signup fallback). Errors
surface in the status bar; full tracing on stderr.

### CalDAV (Radicale)

1. Same dialog, with:
   - **Type**:        CalDAV
   - **Label**:       `Local Radicale`
   - **Server URL**:  `http://127.0.0.1:5232/test/`
   - **Username**:    `test`
   - **Password**:    `test`
2. **Add account → Sync now**.

A first sync auto-discovers the user's principal collection. The
sidebar's right-click on an account header → **New list…**
creates a fresh CalDAV calendar via `MKCALENDAR`.

To add more Radicale users, edit `radicale-config/users` and
append `htpasswd -B`-style bcrypt entries.

## Re-running the Etebase bootstrap

The `etebase-bootstrap` sidecar runs automatically on `up` and
exits 0 after `alice` exists. To re-run manually (e.g. after
changing the test password or migrating to a new server URL):

```sh
docker compose -f docker-compose.test-servers.yml run --rm etebase-bootstrap
```

To bootstrap a *different* user without rebuilding the sidecar
image, override the env vars inline:

```sh
docker compose -f docker-compose.test-servers.yml run --rm \
    -e ETEBASE_TEST_USER=bob \
    -e ETEBASE_TEST_EMAIL=bob@example.com \
    -e ETEBASE_TEST_PASSWORD=bobpw \
    etebase-bootstrap
```

The script logs `bootstrap: <user> already exists and login
works — nothing to do` when the target user is already set up,
or `bootstrap: signed up <user>` when it created the row.

## Security caveats

- Both services bind to `127.0.0.1` only — neither container is
  reachable from anything but the loopback interface.
- Default credentials are intentionally weak. The Etebase Django
  superuser password looks long but it's checked into the
  compose file; rotate it before pointing anything that matters
  at the box.
- Volumes (`etebase_data`, `radicale_data`) persist across
  `docker compose down`. Use `down -v` to wipe state.
- The auto-signup fallback in the desktop client is gated on
  loopback addresses only. Production / public Etebase servers
  never see signup attempts from us.

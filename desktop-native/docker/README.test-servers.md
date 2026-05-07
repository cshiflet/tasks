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
| Etebase  | `http://127.0.0.1:3735/admin/` | `admin` (Django superuser) | `pEjQEwKLd%o^gcLUP#VncOl@&LBn6@` |
| Radicale | `http://127.0.0.1:5232/test/`  | `test`        | `test`                            |
| Radicale | `http://127.0.0.1:5232/.web/`  | `test`        | `test`                            |

The Etebase service ships with `AUTO_SIGNUP=true`, so anonymous
clients can call the public signup endpoint. The desktop client
detects when the server URL is on a loopback address
(`127.0.0.1` / `localhost` / `[::1]`) and falls back to
`Account::signup` on first sign-in if the user doesn't exist
yet — meaning **`alice` is created on demand** the first time the
desktop client syncs against it. No Django-shell dance required.

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

## Adding the Etebase user manually (rarely needed)

The auto-signup fallback means most users never need this. If
you're testing the **password-only** path (e.g. a custom
`with_signup_fallback(false)` build), or pointing a different
client at the server, create `alice` out-of-band:

```sh
docker compose -f docker-compose.test-servers.yml exec etebase \
    /etebase/manage.py shell -c "
from django.contrib.auth import get_user_model
U = get_user_model()
u, _ = U.objects.get_or_create(username='alice', defaults={'email': 'alice@example.com'})
u.set_password('alicepw'); u.save()
print('alice ready')
"
```

The shell path inside the locally-built image is
`/etebase/manage.py`; it changed from `/etebase-server/manage.py`
when we moved off the upstream Docker Hub image. If a future
upstream bump rearranges things, check `etebase/UPSTREAM.md` for
the current layout.

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

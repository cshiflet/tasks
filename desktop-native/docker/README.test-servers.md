# Local sync test servers

`docker-compose.test-servers.yml` brings up two local sync targets
the desktop client can talk to without registering anywhere or
hitting the public internet.

```sh
cd desktop-native/docker
docker compose -f docker-compose.test-servers.yml up -d --build
```

The first `up` builds the Etebase image from upstream source
(there's no Docker Hub image for the 2.x server) — expect ~2-3
minutes of clone + pip install. Subsequent runs reuse the cached
image; pass `--build` again whenever you want to refresh.

The Etebase image pins upstream to `v0.13.0` because newer master
revs of `etesync/server` have shipped without `etebase_server/wsgi.py`,
which Django's runserver needs. Override with
`--build-arg ETEBASE_REF=…` if you want to try a different tag.

The compose stack uses two services on dedicated localhost ports:

| Service  | Port  | URL                          | Desktop-client account | Web admin              |
|----------|-------|------------------------------|------------------------|------------------------|
| Etebase  | 3735  | `http://127.0.0.1:3735`      | `alice / alicepw`      | `admin / changeme`     |
| Radicale | 5232  | `http://127.0.0.1:5232/test/`| `test / test`          | `http://127.0.0.1:5232/.web/` |

Both desktop-client accounts are pre-provisioned on first start
of their containers — no `docker compose exec` step is required.

## Verify the stack is reachable

```sh
# Etebase — any HTTP response means the Django app is up.
curl -sSI http://127.0.0.1:3735/ | head -1
# Radicale — same idea.
curl -sSI http://127.0.0.1:5232/ | head -1
```

If either returns "connection refused" or hangs, check:

```sh
docker compose -f docker-compose.test-servers.yml ps
docker compose -f docker-compose.test-servers.yml logs etebase
docker compose -f docker-compose.test-servers.yml logs radicale
```

## Connecting from the desktop client

1. Launch the client (`cargo run -p tasks-ui` from `desktop-native/`).
2. Open **Settings → Accounts → Add account**.
3. Fill the row using the credentials in the table above.
4. Click **Add account**.
5. Click **Sync now** on the row that appears.

A successful sync flips the row's badge from `Idle` →
`Syncing…` → `Synced (N↓ / 0↑)`, and pulled calendars / tasks
land in the sidebar + list pane. Errors surface in the
bottom-right status bar; check the running tracing log on stderr
for the full message.

Tear-down (wipe state):

```sh
docker compose -f docker-compose.test-servers.yml down -v
```

Tear-down without losing data:

```sh
docker compose -f docker-compose.test-servers.yml down
```

## Etebase setup notes

The entrypoint script provisions two accounts on first start:

- **Django superuser** (`admin / changeme`) — for the `/admin/`
  web UI only. Not the account the desktop client signs in as.
  Driven by the `DJANGO_SUPERUSER_*` env vars in the compose
  file.
- **Etebase test user** (`alice / alicepw`) — what the desktop
  client uses on the Accounts pane. Driven by
  `ETEBASE_TEST_USER` / `ETEBASE_TEST_PASSWORD` /
  `ETEBASE_TEST_EMAIL` in the compose file. Override either set
  by editing the values there and re-running
  `docker compose up -d --build`; first-time provisioning is
  idempotent so existing usernames are left untouched.

The legacy `manage.py etebase-server create-user` command isn't
shipped in v0.13.0 (it landed in a later upstream rev), so the
entrypoint creates the underlying Django auth user directly via
`manage.py shell`. The Etebase signup flow on the client's first
connect attaches the per-user info / pubkey rows around it.

The image also includes a stand-in `etebase_server/wsgi.py` —
v0.13.0's `WSGI_APPLICATION = 'etebase_server.wsgi.application'`
setting is left dangling by the upstream tarball, so the
entrypoint synthesizes a minimal Django boilerplate WSGI module
on start if one isn't already on disk.

## Radicale setup notes

The htpasswd file ships with one bcrypt-hashed user: `test / test`.
To add more users, edit `radicale-config/users` and append lines in
the format `username:$2b$…` (use `htpasswd -B`). The bcrypt hash
shipped here is *intentionally weak* — never reuse it on a
publicly reachable server.

Once Radicale is up, point the desktop client at:

- **Type**:        CalDAV
- **Server URL**:  `http://127.0.0.1:5232/test/`
- **Username**:    `test`
- **Password**:    `test`

A first sync auto-discovers the user's principal collection;
create a calendar via the Radicale web UI at
`http://127.0.0.1:5232/.web/` if you want one to exist before
the client connects.

## Security caveats

- Both services bind explicitly to `127.0.0.1` so the containers
  can't be reached from anything but the loopback interface.
- Default credentials are deliberately weak — these compose files
  are for local iteration only. Don't reuse them anywhere a real
  user might point at the box.
- Volumes (`etebase_data`, `radicale_data`) persist across
  `docker compose down`. Use `down -v` to wipe state.

# Local sync test servers

`docker-compose.test-servers.yml` brings up two local sync targets
the desktop client can talk to without registering anywhere or
hitting the public internet.

```sh
cd desktop-native/docker
docker compose -f docker-compose.test-servers.yml up -d
```

The Etebase service runs the community-maintained
`victorrds/etesync:alpine` image (no local build step needed).
The Radicale service uses `tomsquest/docker-radicale:latest`.

The compose stack uses two services on dedicated localhost ports:

| Service  | Port  | URL                          | Desktop-client account | Web admin              |
|----------|-------|------------------------------|------------------------|------------------------|
| Etebase  | 3735  | `http://127.0.0.1:3735`      | `alice / alicepw` *    | `admin / SUPER_PASS`   |
| Radicale | 5232  | `http://127.0.0.1:5232/test/`| `test / test`          | `http://127.0.0.1:5232/.web/` |

\* Etebase's `alice` user must be created **once** before first sync — see
"Adding the Etebase test user" below. Radicale's `test` user is in the htpasswd
file we ship.

## Verify the stack is reachable

```sh
# Etebase — any HTTP response means the server is up.
curl -sSI http://127.0.0.1:3735/ | head -1
# Radicale — same idea.
curl -sSI http://127.0.0.1:5232/ | head -1
```

If either returns "connection refused" or hangs:

```sh
docker compose -f docker-compose.test-servers.yml ps
docker compose -f docker-compose.test-servers.yml logs etebase
docker compose -f docker-compose.test-servers.yml logs radicale
```

## Adding the Etebase test user

The compose file sets `AUTO_SIGNUP=true` on the Etebase service, which
opens the public `/api/v1/authentication/signup/` endpoint to anonymous
clients. The desktop client doesn't currently call that endpoint
(`tasks-sync::EteSyncProvider` only does `Account::login`), so the
`alice` user has to be created out-of-band the first time.

Two practical options:

### Option A — Django shell (one-shot)

Creates the Django auth user only. Etebase's first login will
attach the per-user crypto material around it.

```sh
docker compose -f docker-compose.test-servers.yml exec etebase \
    /etebase-server/manage.py shell -c "
from django.contrib.auth import get_user_model
U = get_user_model()
u, created = U.objects.get_or_create(
    username='alice',
    defaults={'email': 'alice@example.com'})
u.set_password('alicepw'); u.save()
print('alice ready (created=%s)' % created)
"
```

(The shell path inside the `victorrds/etesync:alpine` image is
`/etebase-server/manage.py`. Adjust if upstream rearranges.)

### Option B — Direct API signup (if you have an Etebase client at hand)

Any Etebase client (the Android app, the Etebase web UI, or the
`etebase` CLI) pointed at `http://127.0.0.1:3735` will sign up
`alice` automatically because of `AUTO_SIGNUP=true`. This is the
flow the desktop will use once `EteSyncProvider::connect`'s
fall-back-to-signup-on-`UserNotFound` path lands.

### Connecting from the desktop client

1. Launch the client (`cargo run -p tasks-ui` from `desktop-native/`).
2. Open **Settings → Accounts → Add account**.
3. Fill the row using the credentials in the table above:
   - **Type**:       EteSync
   - **Server URL**: `http://127.0.0.1:3735`
   - **Username**:   `alice`
   - **Password**:   `alicepw`
4. Click **Add account**.
5. Click **Sync now** on the row that appears.

A successful sync flips the row's badge from `Idle` → `Syncing…`
→ `Synced (N↓ / 0↑)`. Pulled calendars / tasks land in the
sidebar + list pane. Errors surface in the bottom-right status
bar; check the running tracing log on stderr for the full
message.

Tear-down (wipe state):

```sh
docker compose -f docker-compose.test-servers.yml down -v
```

Tear-down without losing data:

```sh
docker compose -f docker-compose.test-servers.yml down
```

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

A first sync auto-discovers the user's principal collection; the
desktop's right-click → "New list…" affordance on the account header
creates a fresh CalDAV calendar via MKCALENDAR.

## Security caveats

- Both services bind explicitly to `127.0.0.1` so the containers
  can't be reached from anything but the loopback interface.
- Default credentials are deliberately weak — these compose files
  are for local iteration only. Don't reuse them anywhere a real
  user might point at the box.
- Volumes (`etebase_data`, `radicale_data`) persist across
  `docker compose down`. Use `down -v` to wipe state.

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

| Service  | Port  | Wire-up                                                                  |
|----------|-------|--------------------------------------------------------------------------|
| Etebase  | 3735  | `http://127.0.0.1:3735` — Django admin `admin / changeme`                |
| Radicale | 5232  | `http://127.0.0.1:5232/` — user `test / test`                            |

Tear-down (wipe state):

```sh
docker compose -f docker-compose.test-servers.yml down -v
```

Tear-down without losing data:

```sh
docker compose -f docker-compose.test-servers.yml down
```

## Etebase setup notes

The Django superuser (`admin`) is provisioned automatically from
the env vars in the compose file the first time the container
starts. That account is for the Django admin UI, **not** for the
desktop client to sign in as — the EteSync protocol uses its own
user objects.

Create a *test user* via the management command shipped with the
upstream project:

```sh
docker compose -f docker-compose.test-servers.yml exec etebase \
    ./manage.py etebase-server create-user alice alice@example.com
```

The command prompts for a password. Then point the desktop
client's Accounts pane at:

- **Type**:        EteSync
- **Server URL**:  `http://127.0.0.1:3735`
- **Username**:    `alice`
- **Password**:    whatever password you set above

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

# Local sync test servers

`docker-compose.test-servers.yml` brings up two local sync targets
the desktop client can talk to without registering anywhere or
hitting the public internet.

```sh
cd desktop-native/docker
docker compose -f docker-compose.test-servers.yml up -d
```

The compose stack uses two services on dedicated localhost ports:

| Service  | Port | Wire-up                                                                  |
|----------|------|--------------------------------------------------------------------------|
| Etebase  | 8001 | `http://127.0.0.1:8001` — superuser `admin / changeme`                   |
| Radicale | 5232 | `http://127.0.0.1:5232/` — user `test / test`                            |

Tear-down (and wipe volumes):

```sh
docker compose -f docker-compose.test-servers.yml down -v
```

## Etebase setup notes

The `etesync/server` image creates the Django superuser on first
start using the `SUPER_USER` + `DJANGO_SUPERUSER_PASSWORD` env vars
in the compose file. To create a *test user* (the account the
desktop client signs in as), use the Django admin at
`http://127.0.0.1:8001/admin/` or the `etebase-cli` from inside
the running container:

```sh
docker compose -f docker-compose.test-servers.yml exec etebase \
    ./manage.py etebase-server create-user alice alice@example.com
```

Then point the desktop client's Accounts pane at:

- **Type**:        EteSync
- **Server URL**:  `http://127.0.0.1:8001`
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

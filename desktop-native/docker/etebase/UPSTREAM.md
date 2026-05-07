# Etebase Docker — vendored from upstream

This directory carries a snapshot of the
[`victor-rds/docker-etebase`](https://github.com/victor-rds/docker-etebase)
project (the source for the `victorrds/etesync` images on Docker
Hub). The image is built locally rather than pulled so we can:

- Pin to a specific upstream commit.
- Modify the Dockerfile / entrypoint without forking.
- Avoid surprise behaviour changes on a `:latest` tag rotation.

## What's vendored

| File                     | Source on upstream                |
|--------------------------|-----------------------------------|
| `Dockerfile`             | `tags/alpine/Dockerfile`          |
| `context/entrypoint.sh`  | `context/entrypoint.sh`           |
| `server_version`         | `server_version`                  |

The `slim` and `base` Dockerfile variants from upstream aren't copied —
the compose stack uses the alpine image.

## Vendored from

- **Source URL**: `https://github.com/victor-rds/docker-etebase`
- **Commit SHA**: `d5895d61dfd5e34ca90efe9dd0efcb39c4c37ec1`
- **`server_version`**: `v0.14.2` (Etebase server version baked in by upstream)
- **Date copied**: 2026-05-07

## Local modifications

None today. All files are byte-identical to upstream at the SHA
above. If we ever diverge, list the changes here so the next bump
can re-apply them after a fresh copy.

## Bumping to a newer upstream

```sh
cd /tmp
rm -rf docker-etebase-upstream
git clone --depth 1 https://github.com/victor-rds/docker-etebase.git docker-etebase-upstream
cd docker-etebase-upstream
git rev-parse HEAD                # note the SHA for UPSTREAM.md
diff -ru tags/alpine/Dockerfile        <repo>/desktop-native/docker/etebase/Dockerfile
diff -ru context/entrypoint.sh         <repo>/desktop-native/docker/etebase/context/entrypoint.sh
# review the deltas, copy the upstream files in, re-apply any
# modifications listed above, update the SHA + date in UPSTREAM.md,
# commit with a message that records the new SHA + the Etebase
# server version baked into the new tarball.
```

If you want a periodic upstream-watch nudge, the lightest version
is a `.github/workflows/etebase-upstream-check.yml` cron job that
diffs the vendored files against upstream and opens an issue when
they drift. Optional — manual bumps every couple of months are
generally enough since the upstream surface is small.

## Why not a git submodule

Submodule workflows add ceremony (collaborators need
`clone --recursive`, easy to commit a stale pointer) for marginal
benefit when the only thing the link buys is "easy to bump". A
plain copy + recorded SHA + diff-on-bump is shorter for everyone.

## Building / running

The `etebase` service in `../docker-compose.test-servers.yml`
points at this directory via `build: ./etebase`, so the normal
`docker compose -f docker-compose.test-servers.yml up -d --build`
picks it up automatically.

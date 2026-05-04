#!/bin/sh
# Bring up an Etebase test server: migrate the DB, optionally
# provision a Django superuser, then serve via the Django dev
# server on port 3735.
#
# Idempotent — safe to re-run on a container that already has
# data; migrations no-op and the superuser block guards against a
# duplicate username.

set -eu

cd /app

# 1. Apply the latest migrations. No-op once the DB is current.
./manage.py migrate --noinput

# 2. Provision an admin user if env vars are set and they don't
#    already exist. Lets the docker-compose env wire up auth
#    without manual setup; users without `DJANGO_SUPERUSER_*` set
#    skip this block.
if [ -n "${DJANGO_SUPERUSER_USERNAME:-}" ] && [ -n "${DJANGO_SUPERUSER_PASSWORD:-}" ]; then
    ./manage.py shell -c "
from django.contrib.auth import get_user_model
U = get_user_model()
u = '${DJANGO_SUPERUSER_USERNAME}'
if not U.objects.filter(username=u).exists():
    U.objects.create_superuser(
        u,
        '${DJANGO_SUPERUSER_EMAIL:-admin@example.com}',
        '${DJANGO_SUPERUSER_PASSWORD}')
    print(f'Created superuser {u}')
else:
    print(f'Superuser {u} already exists')
"
fi

# 3. Serve. `--insecure` lets runserver hand back static files
#    even though we don't run `collectstatic` — fine for a local
#    test box, never use this layout in production.
exec ./manage.py runserver --insecure 0.0.0.0:3735

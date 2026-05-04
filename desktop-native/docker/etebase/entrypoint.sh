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

# 0. WSGI shim. Multiple upstream `etesync/server` tags ship with
#    `settings.WSGI_APPLICATION = 'etebase_server.wsgi.application'`
#    but no `etebase_server/wsgi.py`, leaving Django's runserver
#    bailing out with `ModuleNotFoundError: No module named
#    'etebase_server.wsgi'`. Detect the project package via the
#    DJANGO_SETTINGS_MODULE that manage.py exports and synthesize a
#    minimal WSGI module if it's missing.
PKG=$(grep -oE "DJANGO_SETTINGS_MODULE['\"]?,[[:space:]]*['\"][^'\"]+['\"]" manage.py 2>/dev/null \
        | head -1 \
        | sed -E "s/.*['\"]([^'\"]+)\.settings['\"].*/\1/")
if [ -z "${PKG:-}" ] || [ ! -d "$PKG" ]; then
    # Fall back to the historical default; covers the case where
    # manage.py is shaped slightly differently across tags.
    PKG=etebase_server
fi
if [ -d "$PKG" ] && [ ! -f "$PKG/wsgi.py" ]; then
    cat > "$PKG/wsgi.py" <<EOF
import os
from django.core.wsgi import get_wsgi_application
os.environ.setdefault('DJANGO_SETTINGS_MODULE', '${PKG}.settings')
application = get_wsgi_application()
EOF
    echo "entrypoint: synthesized $PKG/wsgi.py shim"
fi

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

# 2b. Provision a normal Etebase test user if env vars are set.
#     v0.13.0 doesn't ship a `manage.py etebase-server create-user`
#     command (that landed in a later upstream rev), so we create
#     the underlying Django auth user directly via `manage.py
#     shell`. The Etebase signup flow on first client connect will
#     attach the user-info / pubkey rows it needs around it.
if [ -n "${ETEBASE_TEST_USER:-}" ] && [ -n "${ETEBASE_TEST_PASSWORD:-}" ]; then
    ./manage.py shell -c "
from django.contrib.auth import get_user_model
U = get_user_model()
u = '${ETEBASE_TEST_USER}'
if not U.objects.filter(username=u).exists():
    U.objects.create_user(
        u,
        '${ETEBASE_TEST_EMAIL:-${ETEBASE_TEST_USER}@example.com}',
        '${ETEBASE_TEST_PASSWORD}')
    print(f'Created test user {u}')
else:
    print(f'Test user {u} already exists')
"
fi

# 3. Serve. `--insecure` lets runserver hand back static files
#    even though we don't run `collectstatic` — fine for a local
#    test box, never use this layout in production.
exec ./manage.py runserver --insecure 0.0.0.0:3735

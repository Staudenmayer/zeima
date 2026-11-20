#!/bin/sh
# Second database for Keycloak next to the API's (SPEC §7.1). Runs once, when
# the data directory is created.
set -e
# The password goes in as a psql variable and is quoted by psql (:'pw'), not by the shell.
psql -v ON_ERROR_STOP=1 -v pw="$KEYCLOAK_DB_PASSWORD" -U "$POSTGRES_USER" -d postgres <<'SQL'
CREATE USER keycloak WITH PASSWORD :'pw';
CREATE DATABASE keycloak OWNER keycloak;
SQL

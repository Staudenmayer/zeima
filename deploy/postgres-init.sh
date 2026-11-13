#!/bin/sh
# Second database for Keycloak next to the API's (SPEC §7.1). Runs once, when
# the data directory is created.
set -e
psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d postgres <<SQL
CREATE USER keycloak WITH PASSWORD '$KEYCLOAK_DB_PASSWORD';
CREATE DATABASE keycloak OWNER keycloak;
SQL

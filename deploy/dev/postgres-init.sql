-- Two databases on one server, as in the self-hosted setup (SPEC §7.1):
-- `zeima` for the API, `keycloak` for Keycloak. `zeima` is created by
-- POSTGRES_DB.
CREATE USER keycloak WITH PASSWORD 'keycloak';
CREATE DATABASE keycloak OWNER keycloak;

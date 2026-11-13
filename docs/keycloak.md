# Keycloak: realm, social login, theme

The realm lives in `deploy/keycloak/realm-zeima.json` (realm `zeima`, clients
`zeima-desktop`, `zeima-api`, `zeima-api-admin`, `zeima-swagger`, `zeima-web`).
Values that differ per installation are `${NAME:default}` placeholders that
Keycloak fills from its environment when it imports the file (SMTP, URLs,
the admin client secret; see `deploy/docker-compose.yml`).

## Changing the realm

Keycloak imports the file only on the **first** start. Later changes reach a
running installation with `deploy/keycloak/apply.sh`, which runs
[keycloak-config-cli](https://github.com/adorsys/keycloak-config-cli). Run it
with the same `ZEIMA_*` values the Keycloak container has. CI does this against
the self-hosted stack and then signs in (job `selfhosted`).

## Sign-in page theme

`deploy/keycloak/themes/zeima/login` is mounted into Keycloak
(`/opt/keycloak/themes/zeima`) and selected with `"loginTheme": "zeima"`. It
extends the standard theme (`keycloak.v2`) with the Zeima logo, colors and
clearer texts; German and English come from the realm's locales.

## Google and Microsoft sign-in (cloud)

These need an OAuth client registered with each provider, which only the
operator of the installation can do. Steps:

1. Register an OAuth client with the provider:
   - **Google**: Cloud console → APIs & Services → Credentials → OAuth client
     (type Web). Redirect URI: `https://<ID_URL>/realms/zeima/broker/google/endpoint`.
   - **Microsoft**: Entra ID → App registrations (multi-tenant + personal
     accounts). Redirect URI: `https://<ID_URL>/realms/zeima/broker/microsoft/endpoint`.
2. Keycloak admin console → realm `zeima` → Identity providers → add the
   provider and enter the client id and secret. (Alternatively add an
   `identityProviders` entry to the realm file with the secret as a
   `${ZEIMA_GOOGLE_CLIENT_SECRET}` placeholder and apply it with
   `apply.sh`; keep the secret out of the repository.)
3. Set "Trust email" only for providers whose addresses are verified, and keep
   "First login flow" on the default so a Google user with the same address as
   an existing user is asked to link accounts instead of silently merged.

Self-hosted installations use Keycloak's own user federation (LDAP/AD) or
identity providers instead (SPEC §7.2).

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

## Organizations: roles, departments and the API's copy

Each customer organization is a Keycloak **Organization**. Roles and
departments are groups, which the access token lists in its `groups` claim:

```
/orgs/<orgId>/roles/<owner|admin|manager|member>   the person's role
/orgs/<orgId>/groups/<department>[/<sub>]          departments
```

The API creates and changes all of this through the service account
`zeima-api-admin` (roles `manage-users`, `view-users`, `query-groups`,
`manage-organizations`, `view-organizations`; set by the realm file), so admins
never need the Keycloak console. It keeps a copy of members, roles, departments
and who is in them in its own database, and checks that copy against Keycloak:

- on every sign-in (the person's own row),
- **every `RECONCILE_INTERVAL_HOURS` hours** (default 24) and on demand with
  `POST /v1/orgs/:id/reconcile` (owners and admins), which is the way to pick up
  changes made in the Keycloak console,
- when Keycloak says something changed: `POST /v1/internal/keycloak-events`,
  switched on by setting `KEYCLOAK_WEBHOOK_SECRET` (16 or more characters).

The webhook is for an event listener you add to Keycloak (a small provider
that posts the events of interest). The body is JSON like
`{"type":"GROUP_MEMBERSHIP","orgId":"<uuid>"}` or `{"type":"UPDATE_PROFILE","userId":"<uuid>"}`,
signed with HMAC-SHA256 of the exact bytes sent, in the header
`x-zeima-signature: sha256=<hex>`. The event only says where to look: the API
re-reads that organization (or the person's organizations) from Keycloak, so a
forged or replayed event can do no harm beyond an extra check. Without a
listener the nightly check keeps the copy right. This repository does not ship
the listener.

Invitations are sent by Keycloak, so it needs a working SMTP server
(`ZEIMA_SMTP_*`; the dev stack uses Mailpit). With Organizations switched on,
Keycloak's sign-in asks for the address first and the password on a second page.

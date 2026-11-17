# Keycloak: realm and theme

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

## Theme

`deploy/keycloak/themes/zeima` is mounted into Keycloak
(`/opt/keycloak/themes/zeima`) and selected by the realm
(`loginTheme`, `accountTheme` and `emailTheme` in `realm-zeima.json`). All
three give Keycloak the look of the app: Roboto, the neutral surfaces, the dark
primary button and the 10 px rounding from `src/style.css`, light and dark
(the pages follow the visitor's system setting), with the Zeima mark and name
in the header.

| Type | Folder | Based on | What it changes |
|---|---|---|---|
| Sign-in | `login` | `keycloak.v2` | The pages for sign-in, registration, password reset and so on: logo, card, fields, buttons; clearer texts in `messages/`. |
| Account | `account` | `keycloak.v3` | The account console (profile, security, sessions): header, navigation, fields, buttons. |
| Email | `email` | `base` | A Zeima frame around Keycloak's emails (verification, password reset). The texts stay Keycloak's. |

The styles are plain CSS (`resources/css/zeima.css` in `login` and `account`)
on top of the parent theme; the Roboto font files and the logo are bundled, so
no page loads anything from another server. To change the colors, edit the
variables at the top of those two files (they mirror `src/style.css`). The dev
stack (`compose.dev.yml`) serves the folder directly, so a reload shows the
change; an existing realm takes the theme names with `apply.sh` or in the admin
console (Realm settings → Themes).

## Other ways to sign in

People sign in with an email address and a password (self-registration, with
email verification, is on in the realm). Zeima does not ship Google or
Microsoft sign-in. An installation that wants its own directory connects it in
Keycloak: user federation (LDAP/AD) or identity providers (Entra ID, Google
Workspace, SAML/OIDC); see SPEC §7.2.

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

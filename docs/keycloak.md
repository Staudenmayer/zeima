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

The webhook is called by the **event listener** that ships with the stack
(`deploy/keycloak/listener`, a small Keycloak provider, id `zeima-webhook`). The
Keycloak image built from `deploy/keycloak/Dockerfile` contains it, and the realm
file switches it on (`eventsListeners`). It needs the API's address and the secret,
which the compose files pass as `ZEIMA_WEBHOOK_URL` and `ZEIMA_WEBHOOK_SECRET`; the
production stack sets them when `KEYCLOAK_WEBHOOK_SECRET` is set (16 characters or
more, the same on the API), and without a secret the listener does nothing.

It tells the API about:

| In Keycloak | Sent |
|---|---|
| A person registers, changes their profile or email, verifies their email, deletes their account | `{"type":"UPDATE_PROFILE","userId":"…"}` |
| An administrator changes a user, or their role mappings | `{"type":"USER","userId":"…"}` |
| A group of an organization (department or role) is created, changed or deleted, or a person is added to or removed from one | `{"type":"GROUP_MEMBERSHIP","orgId":"…"}` (the organization comes from the group's path `/orgs/<orgId>/…`; a group that is already gone is reported by the person, if known) |
| A Keycloak organization, its members or groups change | `{"type":"ORGANIZATION_MEMBERSHIP","orgId":"…"}` |

Each body is JSON, signed with HMAC-SHA256 of the exact bytes sent, in the header
`x-zeima-signature: sha256=<hex>`. The event only says where to look: the API
re-reads that organization (or the person's organizations) from Keycloak, so a
forged or replayed event can do no harm beyond an extra check.

The listener sends a notice only after Keycloak has committed the change (before
that, the API would read the old state), once per change however often it is
repeated in one transaction. It posts from its own thread, so Keycloak never waits
for the API; it tries three times (a refusal such as a wrong secret is not
repeated) and then gives up, and the nightly check catches up. The change was made
in Keycloak, so a missed notice costs only time.

Build and test it with `mvn -f deploy/keycloak/listener/pom.xml verify` (Java 21);
CI does the same. For a Keycloak you run yourself, copy
`target/zeima-keycloak-listener.jar` into its `providers/` folder, set the two
variables, and add `zeima-webhook` to the realm's event listeners (Realm settings →
Events). Admin events must be enabled for the realm (the realm file does).

Invitations are sent by Keycloak, so it needs a working SMTP server
(`ZEIMA_SMTP_*`; the dev stack uses Mailpit). With Organizations switched on,
Keycloak's sign-in asks for the address first and the password on a second page.

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
Microsoft sign-in for everyone. Companies connect their own provider, per
organization, as described next; an installation that wants its whole directory
(LDAP/AD) connects it as described under *Company directory*.

## Single sign-on per organization

An owner or admin (the permission `org.settings`) connects the organization's
own identity provider on the Organization page, tab **Single sign-on**: Microsoft
Entra ID, Google Workspace, any OpenID Connect provider, or a SAML provider. People
whose email address is in the organization's domains are sent to that provider as
soon as they give their address on the sign-in page, and join the organization
when they sign in. Everybody else signs in as before. On the cloud it is an
enterprise-plan feature (`sso`).

How it is built (`server/src/modules/orgs/sso.ts`, `server/src/keycloak/admin.ts`):

- The API creates a Keycloak **identity provider** `sso-<orgId>` (OIDC or SAML;
  Keycloak reads the endpoints and keys from the provider's metadata), hidden from
  the sign-in page, links it to the Keycloak organization, and ties each email
  domain of the organization to it (domain with `identityProviderAlias` and
  `autoRedirect`). Keycloak's identity-first login does the redirect.
- The provider's own address (`redirectUri`, or `samlMetadataUrl` for SAML) is what
  the administrator enters at the provider. `GET /v1/orgs/:id/sso` gives it before
  anything is set up; the client secret is never given back.
- Routes: `GET`, `PUT` and `DELETE /v1/orgs/:id/sso`. Changes are in the activity
  log (`sso.set`, `sso.delete`). Removing it removes the provider and the domains;
  people who joined stay members and keep their accounts.
- The people who join this way are plain members, without an invitation. The seat
  limit applies to them: when Keycloak tells the API about a new member (or the
  nightly check finds one), a person beyond the free seats is taken out of the
  organization again and the activity log says `member.rejected` (reason
  `seat_limit`). A person with a pending invitation keeps the seat the invitation
  holds, and nobody who is already in is removed when seats shrink.
- The email address is not trusted as proof of an account: `trustEmail` is off, so
  Keycloak still confirms an address that matches an existing account.

Two safeguards, because the provider's address is read by Keycloak from inside your
network and because an organization could otherwise claim someone else's domain:

- **Addresses**: only public https addresses are accepted (no user name in the
  address, no `localhost`, private networks or names that resolve to them). For
  development, `SSO_ALLOW_PRIVATE_URLS=true` allows others.
- **Domain proof**: where `SSO_VERIFY_DOMAINS` is on (default in the cloud, off
  when self-hosted), the organization must publish a TXT record
  `_zeima-challenge.<domain>` with the value the API shows for the organization
  (derived from a secret only the API has), for every domain it signs people in
  for. The page shows the value.

The service account needs the realm roles `manage-identity-providers` and
`view-identity-providers` (the realm file has them; apply it again, see
*Changing the realm*). `deploy/e2e/org-smoke.mjs` with `E2E_SSO=1` sets it up
against a running stack, using Keycloak's master realm as the provider, and checks
that a matching address is redirected to it.

## Provisioning with SCIM

Instead of waiting for people to sign in, the identity provider can create, rename,
disable and remove them itself (SCIM 2.0, users only). On the same tab an owner or
admin makes a **key** (up to five; shown once, only a hash is kept) and gives the
provider the SCIM address (`<PUBLIC_URL>/v1/scim/v2`) and the key as secret token.
`server/src/modules/orgs/scim.ts`:

- `GET /v1/orgs/:id/scim`, `POST /v1/orgs/:id/scim/tokens`, `DELETE /v1/orgs/:id/scim/tokens/:keyId`
  make the keys (permission `org.settings`, enterprise plan on the cloud, in the activity log).
- `/v1/scim/v2/Users` (list with `userName eq "…"`, get, create, replace, patch, delete),
  `ServiceProviderConfig`, `ResourceTypes`, `Schemas`, with a key in `Authorization: Bearer`.
- Only people whose address is in a domain the organization set up for single sign-on
  are created or changed; owners are never changed; a person who is already an account
  of the installation is added to the organization, not duplicated.
- `active: false` disables the account in Keycloak and ends their sessions; the API also
  refuses their tokens within seconds. Delete takes the person out of the organization
  (the account stays). The seat limit applies to creating people (409).

## Signing people out

A disabled account (by an administrator, by SCIM, or in Keycloak) stops working
without waiting for its token to run out: the listener tells the API, the API checks
the organization again, ends the person's Keycloak sessions, and refuses a request
whose account is disabled (the state is remembered for 15 seconds). OIDC providers
are set up with back-channel logout, so the provider can end Keycloak sessions
itself. A provider that only disables a person, without SCIM, is noticed when their
session next asks the provider.

## Company directory (LDAP / Active Directory)

For self-hosted installations whose people already have directory accounts: the
people who run the installation (realm role `platform-admin`) connect the directory
in the same place (Organization page, Single sign-on tab) or with `GET`/`PUT`/`DELETE
/v1/admin/ldap`, `POST /v1/admin/ldap/test` and `/v1/admin/ldap/sync`
(`server/src/modules/directory/routes.ts`). The routes exist only on self-hosted
servers. Keycloak checks the connection first, then keeps one user federation
provider `zeima-ldap`: read only, people imported on first sign-in and every day,
the directory's email address trusted (so invitations work). People sign in on the
Keycloak page with their directory name and password; an owner then invites their
email address. The service account needs `manage-realm` for this (user federation
is a realm setting); a cloud installation does not use the routes.

## Organizations: roles, departments and the API's copy

Each customer organization is a Keycloak **Organization**. Roles and
departments are groups, which the access token lists in its `groups` claim:

```
/orgs/<orgId>/roles/<owner|admin|manager|member>   the person's role
/orgs/<orgId>/groups/<department>[/<sub>]          departments
```

The API creates and changes all of this through the service account
`zeima-api-admin` (roles `manage-users`, `view-users`, `query-groups`,
`manage-organizations`, `view-organizations`, `manage-identity-providers`, `manage-realm` (company directory),
`view-identity-providers`; set by the realm file), so admins
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

# Demo data for development

Two scripts fill a development setup with data, so every feature can be tried
without typing it in. They are for development only; do not run them against a
real installation.

| Script | Fills | Needs |
|---|---|---|
| `crates/zeima-core/examples/seed.rs` | the local database: projects, tags, cost centers, three weeks of entries, absences, a holiday | the app closed |
| `server/scripts/seed-org.ts` | an organization: people, departments, shared data, entries, absences, meetings | the dev stack running, and an organization you own |

Both only add data. Run the local one twice and everything exists twice. The
organization one gives every row a fixed id, so running it again updates the
same rows.

## 1. Local database

Close the app, then from the repository root:

```bash
DB=~/.local/share/com.zeima.zeima/zeima.db      # the app's data folder on Linux
cp "$DB" "$DB.backup"                            # optional
cargo run -p zeima-core --features db --example seed -- "$DB"
```

It writes through the app's own code, so the change log and sync state stay
consistent. It adds:

- activities that are missing (Development, Meeting, Review, Planning,
  Support, Documentation, Admin);
- 3 cost centers, one inactive;
- 5 projects: monthly and weekly budgets, a client, a paused one, an archived one;
- 5 tags in two groups;
- entries for the last three weeks (weekdays), with tags, billable settings and
  some cost centers (today only the first three entries);
- 4 absences (sick, half day, vacation, training) and one custom holiday.

Pass `absences` as a second argument to add only the absences and the holiday:

```bash
cargo run -p zeima-core --features db --example seed -- "$DB" absences
```

Meetings are not part of this script: they exist only in an organization's
workspace (see below).

## 2. Organization

Prerequisites:

- the dev stack of `deploy/compose.dev.yml` is up: API on `:3000`, Keycloak on
  `:8080`, Postgres;
- `server/.env` exists (copy `server/.env.example`), including
  `KEYCLOAK_ADMIN_CLIENT_SECRET`;
- you own an organization, and the app has synced it at least once.

Find the two ids the script needs in the app's database:

```bash
sqlite3 ~/.local/share/com.zeima.zeima/zeima.db \
  "select value from local_meta where key = 'account_user_id';   -- OWNER_ID
   select org_id from workspace where kind = 'org';              -- ORG_ID"
```

Run it from `server/`:

```bash
cd server
ORG_ID=<organization id> OWNER_ID=<your user id> \
  node --env-file=.env scripts/seed-org.ts
```

| Variable | Default | Meaning |
|---|---|---|
| `ORG_ID` | – (required) | the organization to fill |
| `OWNER_ID` | – (required) | your user id; you are invited to the meetings |
| `KEYCLOAK_ADMIN_PASSWORD` | `admin` | password of Keycloak's `admin` user (creates the people) |
| `ID_URL` | `http://localhost:8080` | Keycloak |
| `PUBLIC_URL` | `http://localhost:3000` | the API |

It takes a few minutes: the API limits requests per minute, and the script
waits and carries on when it is told to.

What it creates:

- **People** (password `Zeima-seed-1`, sign in with `seed-<name>@example.com`):
  `ada` (admin), `max` (manager), `grace`, `linus`, `margaret`, `alan`,
  `katherine` (members). Departments: Engineering (ada, max, grace, linus),
  Operations (margaret, alan), Sales (katherine).
- **Job descriptions** for the directory, set by each person (e.g. "Senior backend developer").
- **Shared data** (by the admin): activities (the organization's existing ones
  are reused by name, only missing ones are added), 4 cost centers, 5 tags,
  6 projects (one visible to its members only), 3 absence types.
- **Entries:** about three weeks per person, roughly 385 in total.
- **Absences:** pending, approved and rejected requests; an admin decides.
- **Rooms** (by `ada`): Blue room, Green room and Board room, with seats.
- **Meetings** (by `max`, inviting you and others): a daily standup, weekly
  planning (Blue room), a Tuesday/Thursday design sync (Green room), a
  retrospective every second Friday (Board room), a customer demo, a quarterly
  review (Board room) and a cancelled workshop; plus attendee answers. No room
  is booked twice.

Your own entries and meetings in the organization are not created: the script
cannot act as you. Create those in the app. The app pulls the rest the next time
it syncs (within a minute, or restart it).

## Starting over

- Local: close the app and put the `.backup` copy back.
- Organization: delete the organization in the app (owners) and create a new
  one, or remove the `seed-*` users in Keycloak's admin console. The script
  does not delete anything.

## Troubleshooting

- `set ORG_ID and OWNER_ID`: both variables are required.
- `KEYCLOAK_ADMIN_CLIENT_SECRET is not set`: the script uses the server's own
  configuration; check `server/.env`.
- `could not sign in to Keycloak as admin`: set `KEYCLOAK_ADMIN_PASSWORD`.
- Entries without an activity in the app: an older version of the script added
  copies of activities the organization already had. Run the current script
  again; it removes the copies.
- `no such workspace` while syncing: the server lost the workspace the app
  remembers (for example after resetting the dev database). Remove the
  `sync_workspace` and `sync_joined` rows from `local_meta` in the app's
  database; the next sync creates the workspace again.

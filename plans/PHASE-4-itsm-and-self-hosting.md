# Phase 4 — ITSM (asset management) + self-hostable by other people

Two goals that look separate and aren't:

- **ITSM / asset management** — grow past tickets into the assets those
  tickets are about.
- **Self-hostable** — somebody who isn't CSS can stand this up.

They're the same project, because both are gated on one thing this codebase
doesn't have: **a written, versioned contract between client and server.**
Today the contract is "whatever `src/client.rs` guessed and CSSHelpdesk
happens to return." You can't add a second resource family to that safely, and
you certainly can't ask a stranger to implement against it.

## The scope problem, stated plainly

**Almost none of this work happens in this repo.** `csshd` is a client. Asset
management is a data model, a web UI, an importer, and a permissions story —
all of that lives in `css-md/CSSHelpdesk`, which is a separate private repo.
Self-hosting is a Docker image, a config story, pluggable auth, and an OSS
license — also all server-side. This repo's share is maybe 15% of the total:
de-CSS-ing the client, adding `csshd asset …`, and a TUI screen.

So the sequencing below is written for both repos, and the CLI work is
explicitly dependent.

> Revised 2026-09-17 against `css-md/csshelpdesk` @ `9f56469`. The first draft
> of this doc was written without access to the server and guessed at its
> internals; several guesses were wrong, most importantly the assumption that
> asset management didn't exist yet. Corrected throughout.

## Part 1 — The contract (do this first, it unblocks everything)

### 1.1 Write the OpenAPI spec

Publish `openapi.yaml` describing `/api/v1/*` — tickets, comments, users,
auth. Generate it from the Next.js route handlers if you can (zod-to-openapi
if the routes already validate with zod), hand-write it if you can't. Put it
in the server repo, publish it at `/api/v1/openapi.json`, and vendor a copy
here so client changes and contract changes show up in the same diff.

This is the highest-leverage single artifact in the whole plan. It turns
"CSSHelpdesk-compatible" from a vibe into something testable, and it's what
makes a third-party self-host or a second client possible at all.

### 1.2 Add a capability discovery document

`GET /.well-known/helpdesk` — unauthenticated, no secrets:

```json
{
  "product": "CSSHelpdesk",
  "version": "2.4.1",
  "apiVersions": ["v1"],
  "ticket": { "prefix": "CSS", "padding": 5 },
  "modules": ["tickets", "assets", "kb"],
  "statuses":   [{ "id": "OPEN", "label": "Open", "category": "open" }],
  "priorities": [{ "id": "HIGH", "label": "High", "rank": 3 }],
  "bodyFormat": "html",
  "auth": { "deviceCode": { "init": "/api/v1/cli/auth/init",
                            "poll": "/api/v1/cli/auth/poll" } }
}
```

**This reverses a Phase 0 decision on purpose.** Phase 0 said "we don't need
`.well-known` at all" — and it was right, *for auth*: the CLI shouldn't be
fetching tenant IDs and client IDs, and it still won't. This document carries
no secrets and exists for a different reason: capability discovery. A client
that can't ask "what's your ticket prefix, which modules do you run, what
statuses exist" has to hardcode the answers, which is exactly the coupling
listed in the assessment (#8-#11).

Client side: fetch on `login`, cache into `config.toml` next to the URL,
re-fetch when the server reports a different `version`, and degrade gracefully
when the endpoint 404s (assume `tickets` only, prefix from the first ticket
number seen).

### 1.3 Cash the discovery document in

- `resolve_ticket` uses `ticket.prefix` / `ticket.padding` instead of `CSS-`
  and `{:0>5}`.
- `format.rs` / `tui.rs` color by `status.category` (open / active / waiting /
  done / cancelled) and `priority.rank`, not by matching literal strings.
  Unknown values render in default style instead of `????`.
- `bodyFormat` picks the renderer; drop the `<!--html-->` sniffing.
- Commands for absent modules hide themselves from `--help` and error with
  "this helpdesk doesn't have assets enabled".

Do 1.3 even if assets never happen. It's the difference between "generic in
the README" and generic.

## Part 2 — Asset management

### 2.1 You already have half of this

The first draft of this plan proposed building an asset system from scratch.
That was wrong — `csshelpdesk` already ships:

- an **`Asset` model** with `AssetStatus` (ACTIVE / INACTIVE / LOST / RETIRED /
  SPARE), Intune + PDQ external IDs, hostname, serial, make/model, OS, assigned
  user, site, `assetTag`, `manualNotes`, and an `extraFields` JSON escape hatch;
- **`/api/v1/assets`**, `/assets/[id]` and `/assets/export`, with
  status/site/assignedUser/search/pagination filters;
- a full **web UI** — list, detail, actions, column filters, search, CSV export,
  sync button;
- **four sync workers** — `intune-sync`, `pdq-sync`, `chromeos-sync`,
  `simplemdm-sync`;
- **`Ticket.assetId`**, so tickets already link to the device they're about, and
  the ticket list already filters by `assetId`;
- **`AuditLog`** with a dedicated `assetId` FK and `entityType: "asset"`, so
  there's already a history spine.

So the real question isn't "how do we build asset management." It's **"what is
missing before this counts as ITSM rather than an MDM mirror?"**

### 2.2 What's actually missing

Everything currently in `Asset` is a *reflection of what an MDM already knows*.
None of it is what a helpdesk actually needs to answer budget and lifecycle
questions. The gaps, roughly in value order:

1. **Procurement and finance fields.** No `purchaseDate`, `purchaseCost`,
   `poNumber`, `supplier`, `warrantyEndsAt`, or funding source. Without these
   you can't do warranty lookup at the point of repair, refresh-cycle
   forecasting, or the grant/capital reporting a non-profit actually needs.
   This is the single highest-value addition and it's ~8 additive nullable
   columns.
2. **Lifecycle states the current enum can't express.** ACTIVE/INACTIVE/LOST/
   RETIRED/SPARE has no way to say *in for repair*, *loaned out*, or *in stock
   vs. deployed* — which is most of what a helpdesk asset workflow is. Extend
   the enum (additive) rather than repurposing `manualNotes`.
3. **Check-out / check-in as a first-class action.** Assignment today is a
   nullable `assignedUserId` you overwrite. There's no loan, no expected-return
   date, no bulk September/June handoff. `AuditLog` captures *that* it changed
   if the route writes an entry, but "who had this in March, and did they
   return it" is a query nobody wants to reconstruct from `beforeState` JSON.
   Consider a purpose-built `AssetAssignment` (open/closed intervals) rather
   than leaning on the generic audit log for a domain workflow.
4. **CSV import.** There's an `/assets/export` and no import. Every self-hoster
   and every non-MDM asset class (monitors, projectors, furniture, AV) starts
   in a spreadsheet. This is the adoption gate.
5. **Non-MDM assets generally.** Every field is oriented around a device that
   Intune or PDQ reports. An asset with no `intuneDeviceId` is a second-class
   citizen. Worth an explicit "manually tracked" path, including generated
   asset tags.
6. **A reconciliation rule.** With four sync sources writing to one row, decide
   and document which fields are sync-authoritative (hostname, OS, serial) and
   which are local-authoritative (assignment, notes, finance, location) — and
   make the UI show which is which. Mixing them silently is how people stop
   trusting the inventory.
7. **Consumables and licenses.** Seat counts and renewal dates, as a separate
   simpler model. Don't force them into `Asset`.

Notably **not** missing, and not worth building: a generic inventory system,
another sync integration, or a separate CMDB. The bones are fine.

### 2.3 What to skip

Do **not** chase ITIL completeness. Change management, problem records, service
catalog and a relationship-graph CMDB are where this turns into a three-year
project. The one Tier-3 item that probably outranks all of them is **SLA
policies with breach timers** — and that belongs to tickets, not assets.

### 2.4 The CLI surface (this repo's actual work)

Cheaper than the first draft assumed, because `/api/v1/assets` already exists
and already has the filters:

```
csshd asset list [--status] [--site] [--assigned me|<user>] [-q]
csshd asset view <tag|serial|hostname|id>
csshd asset history <asset>            # reads /api/v1/audit-log
csshd asset link <asset> <ticket>      # PATCH /tickets/{id} { assetId }
```

`list` and `view` are implementable **today** against the deployed API with no
server change. `csshd view <ticket>` should also render the linked asset — the
detail route already includes `asset: { id, hostname, make, model, assetTag }`.
Check-out/check-in commands wait on 2.2.3.

**TUI note:** the current `Pane` enum (`List | Detail | Search | Help`) is a
flat state machine with ticket geometry baked into `draw()`. Assets need a
screen-level concept above it — `Screen::{Tickets, Assets}` with panes
underneath. Do that refactor as its own commit before adding asset rendering.

## Part 3 — Actually self-hostable

### 3.1 Server (the real gate — none of this is in this repo)

Better news than the first draft assumed: there is already a `Dockerfile`, a
`Dockerfile.worker`, a 5 KB `docker-compose.yml`, a 7 KB `.env.example` and a
`railway.toml`. The packaging bones exist. What's missing is that none of it
currently works for someone who isn't CSS.

**1. The migration history is broken, and it silently breaks fresh installs.**
This is the first thing to fix and it's not a nice-to-have.

- `prisma/migrations/` covers **30 tables**; the schema declares **39 models**.
- **15 models exist in no migration at all** — including `CliToken` and
  `CliAuthSession` (so Phase 0 itself), plus `TicketParticipant`,
  `TicketMerge`, `CannedResponse`, `TicketTemplate`, all four Helpbot tables,
  `CredentialMonitor`, `NotificationPreference`/`Log`, the cluster tables.
- The last migration is dated **2026-03-27**. Everything since has gone in via
  `db push`: `.github/workflows/deploy.yml:203` runs
  `prisma db push --skip-generate` on every deploy.
- But `docker-compose.yml:64` starts the app with **`prisma migrate deploy`**.

For CSS's prod box those two paths coexist by accident — `db push` keeps the
live database correct and `migrate deploy` finds nothing to do. For anyone
doing `docker compose up` on an empty database, only the compose path runs, so
they get a schema stuck in March 2026 and an app that dies on the first query
against a missing table. **Self-hosting is currently impossible, and this is
why.** The fix is to squash the current schema into a fresh baseline migration
and make `db push` a dev-only tool.

**2. Pick an OSS license and make the repo public.** Until this happens
everything else here is theoretical. It's a decision, not a task.

**3. The environment surface is ~48 variables, most of them CSS-specific.**
`.env.example` demands Azure AD *and* Azure Graph credentials, Google Workspace
service-account + domain + customer ID, three named Entra group IDs
(`ENTRA_SEIA_GROUP_ID`, `ENTRA_SRIA_GROUP_ID`,
`ENTRA_RESIDENTIAL_SUPERVISORS_GROUP_ID`), PDQ, SimpleMDM, a GCS bucket, SSH
keys for gateways, an AI gateway key, and a separate bridge service with its own
HMAC secret. A self-hoster needs a **tiered** env file: a handful of required
vars (database, Redis, auth secret, URL) and everything else behind an
explicitly-disabled integration.

**4. Pluggable auth.** Entra is load-bearing. Needs generic OIDC plus
email/password or magic-link for shops with no IdP. NextAuth already supports
this; the work is config, not invention.

**5. Make the CSS-specific domains optional modules.** Residential clusters and
house assignments, Protect gateways + Tailscale SSH, the gbridge email service,
`ALLOWED_EMAIL_DOMAIN`, credential monitoring — all valuable to CSS, all
meaningless to a generic install, and several are currently unconditional.

**6. First-run setup wizard**: org name, ticket prefix, first admin, sites. The
`SystemConfig` table already exists to hold the answers.

**7. De-brand and document**: logos, colors, email templates, plus
backup/restore, upgrade, reverse proxy, and SMTP docs. A seeded demo instance is
worth more than any amount of README.

### 3.2 Naming

`csshd` means nothing to anyone outside CSS, and neither does `CSSHelpdesk`.
If this is going public, pick the name **before** tagging a public v1.0 —
you already ship installer scripts and tell people to `cargo install --git`,
so the window where renaming is free is closing.

The binary name is also wired into the keyring service string
(`src/credentials.rs`), the config directory (`org.css-md.csshd` in
`src/config.rs`), both installers, and both workflow files. It's a two-hour
change today and a support burden later.

### 3.3 Client (this repo)

Everything in Part 1.3, plus:
- Rename per 3.2, with a migration that reads the old keyring entry and config
  dir once and moves them.
- Warn (don't block) on a plaintext `http://` helpdesk URL — a bearer token
  over cleartext is worth a line of stderr.
- `csshd completions` — already promised in the README, never built.
- Publish to crates.io once the name is settled.

## Recommended sequencing

| Step | Where | Why it's here |
|---|---|---|
| 0 | csshd | ~~Confirm Phase 0 shipped~~ — done, it has. Smoke-test `login`/`list --mine` against prod now that the `assignedTo` fix has landed. |
| 1 | csshd | Assessment items 1-5 — tests, CI teeth, honest docs. |
| 2 | server | OpenAPI spec + `/.well-known/helpdesk`. |
| 3 | csshd | Consume discovery; delete every hardcoded `CSS-`, status and priority string. |
| 4 | server | **Baseline migration squash** (fresh installs are broken today), then license decision + tiered env + pluggable auth. |
| 5 | server | Asset finance/lifecycle fields + status enum + check-out/in (§2.2). Model, API and UI already exist. |
| 6 | csshd | `asset list`/`view` (possible today), then the `Screen` refactor. |
| 7 | server | CSV import, barcodes, bulk ops, warranty reporting. |
| 8 | — | Stop. Use it for a term. Let real demand pick from Tier 3. |

Steps 2 and 4 are the ones that actually decide whether this becomes a product
other people can run. Steps 5-7 are the ones that make it worth running. The
order matters: assets built before the contract exists will bake in a second
generation of the same CSS-specific coupling this plan exists to remove.

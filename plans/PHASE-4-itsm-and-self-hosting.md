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

> Note for whoever picks this up: the private server repo wasn't reachable from
> the session that wrote this doc, so everything below about CSSHelpdesk's
> internals is inferred from `plans/PHASE-0-helpdesk-bearer-auth.md`,
> `src/client.rs`, and the README's "conscious omissions" list. Verify before
> building.

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

### 2.1 Scope discipline

Do **not** try to be ITIL-complete. "ITSM" as a checkbox list (incident,
problem, change, release, config, service catalog, CMDB, SLA) is how this
turns into a three-year project that never ships. The ordering below front-loads
the part that delivers nearly all the operational value.

### Tier 1 — Inventory + ticket linkage (ship this, then stop and use it)

```prisma
model Asset {
  id            String   @id @default(cuid())
  assetTag      String   @unique      // human/barcode: "CSS-A-00412"
  serial        String?
  category      String                // laptop | tablet | display | printer | network | phone | av | other
  make          String?
  model         String?
  status        String                // IN_STOCK | DEPLOYED | IN_REPAIR | LOANED | RETIRED | LOST
  assignedToId  String?               // -> User
  siteId        String?               // reuse the Site model tickets already have
  location      String?               // room / closet / shelf
  purchaseDate  DateTime?
  purchaseCost  Decimal?
  poNumber      String?
  fundingSource String?               // grants and capital budgets matter in schools/nonprofits
  supplier      String?
  warrantyEndsAt DateTime?
  notes         String?
  customFields  Json?                 // the escape hatch every self-hoster needs on day one
  createdAt     DateTime @default(now())
  updatedAt     DateTime @updatedAt
  @@index([status]) @@index([assignedToId]) @@index([siteId]) @@index([serial])
}

model AssetEvent {                     // append-only. Never update, never delete.
  id        String   @id @default(cuid())
  assetId   String
  type      String   // CHECK_OUT | CHECK_IN | MOVE | STATUS_CHANGE | REPAIR | AUDIT | RETIRE | NOTE
  actorId   String   // who did it
  subjectId String?  // who it was checked out to, for CHECK_OUT
  fromValue String?
  toValue   String?
  note      String?
  at        DateTime @default(now())
  @@index([assetId, at])
}

model AssetTicketLink {
  assetId  String
  ticketId String
  @@id([assetId, ticketId])
}
```

Two things here earn their keep more than anything else:

- **`AssetEvent` is append-only.** Every asset system that stores only current
  state gets asked "who had this in March?" within a year and can't answer.
  Write the history from day one; it's cheap now and impossible to backfill.
- **`AssetTicketLink` is the reason to build this inside the helpdesk** rather
  than buying Snipe-IT and pointing at it. It's what gives you "every ticket
  this device ever generated" on the asset page and "which model accounts for
  40% of our repair tickets" in reporting. If you skip the linkage, you have
  built a worse spreadsheet.

### Tier 2 — The things that decide whether anyone adopts it

- **CSV import/export with a dry-run.** Everyone's inventory starts in a
  spreadsheet. Import is the adoption gate, not a nice-to-have.
- **Barcode / asset-tag printing and scan-to-lookup.** For a 1:1 device
  program this *is* the daily workflow — scan the tag, see the student,
  see the open ticket.
- **Bulk check-out/check-in.** September and June are the whole year.
- **Warranty and EOL reporting** → refresh-cycle forecasting, which is what
  gets the budget conversation.
- **Consumables and licenses** as a separate, simpler model (seat counts,
  renewal dates) — don't force them into `Asset`.

### Tier 3 — Only on demand

CMDB relationships (asset depends-on asset, service → CI), change requests with
approvals and maintenance windows, problem records over recurring incidents,
service catalog. SLA policies are the exception — they belong to tickets, not
assets, and are probably worth more than all of Tier 3 combined.

### 2.2 Sync sources

Don't hardcode an importer. Define a small importer interface (pull → normalize
→ reconcile by serial → report drift) and implement against it. Realistic first
sources for a district: Intune/Entra devices, Google Admin SDK Chrome devices,
Jamf. A self-hoster with none of those still has CSV, which is why CSV comes
first.

Reconciliation rule worth deciding up front: imported fields are
server-authoritative and locally uneditable; everything else (assignment,
location, funding) is local. Mixing them is how these systems become untrusted.

### 2.3 The CLI surface (this repo's actual work)

```
csshd asset list [--status] [--site] [--category] [--assigned me|<user>] [-q]
csshd asset view <tag|serial|id>
csshd asset checkout <asset> --to <user> [--note]
csshd asset checkin  <asset> [--status IN_STOCK]
csshd asset move     <asset> --site <site> [--location <room>]
csshd asset link     <asset> <ticket>
csshd asset history  <asset>
csshd asset import   inventory.csv [--dry-run]
```

`csshd view <ticket>` grows a "Linked assets" block. `--json` throughout.

**TUI note:** the current `Pane` enum (`List | Detail | Search | Help`) is a
flat 4-variant state machine with ticket geometry baked into `draw()`. Assets
need a screen-level concept above it — `Screen::{Tickets, Assets}` with panes
underneath, and `draw()` split per screen. Better to do that refactor as its
own commit before adding asset rendering, not tangled with it.

## Part 3 — Actually self-hostable

### 3.1 Server (the real gate — none of this is in this repo)

1. **Pick an OSS license for CSSHelpdesk and make the repo public.** Until this
   happens, everything else in Part 3 is theoretical. This is the decision, not
   a task.
2. **Docker Compose**: app + Postgres + a single `.env`, `docker compose up`
   to a working instance. This is the bar people judge self-hostability by,
   and they judge it in about ninety seconds.
3. **Pluggable auth.** Entra is currently load-bearing. Needs: generic OIDC
   (issuer/client id/secret from env), plus email+password or magic-link for
   small shops with no IdP. NextAuth supports this; the work is config, not
   invention.
4. **First-run setup wizard**: org name, ticket prefix, first admin, sites.
   Everything the discovery document exposes should be settable here.
5. **Ship Prisma migrations, not `db push`**, with a documented upgrade path.
   Self-hosters upgrade on their own schedule and will be several versions
   behind.
6. **Move CSS-specific domains behind feature flags.** The README's "conscious
   omissions" already names House Assignments as a CSS-only domain with its own
   print-roster UI — that's exactly the kind of thing that should be a disabled
   module in a generic install, alongside the CSS SIS integration and support
   tiers.
7. **De-brand**: logos, colors, copy, and the email templates.
8. **Operational docs**: backup/restore, upgrade, reverse proxy + TLS, SMTP,
   env var reference. Plus a demo instance with seeded data, which is worth
   more than any amount of README.

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
| 0 | both | Confirm Phase 0 shipped and csshd works end-to-end. Everything below assumes a working client. |
| 1 | csshd | Assessment items 1-5 — tests, CI teeth, honest docs. |
| 2 | server | OpenAPI spec + `/.well-known/helpdesk`. |
| 3 | csshd | Consume discovery; delete every hardcoded `CSS-`, status and priority string. |
| 4 | server | License decision + Docker Compose + pluggable auth. The gate for Part 3. |
| 5 | server | Asset Tier 1: model, CRUD, `AssetEvent`, ticket linkage, web UI. |
| 6 | csshd | `Screen` refactor, then `csshd asset …`. |
| 7 | server | Asset Tier 2: CSV import, barcodes, bulk ops, warranty reporting. |
| 8 | — | Stop. Use it for a term. Let real demand pick from Tier 3. |

Steps 2 and 4 are the ones that actually decide whether this becomes a product
other people can run. Steps 5-7 are the ones that make it worth running. The
order matters: assets built before the contract exists will bake in a second
generation of the same CSS-specific coupling this plan exists to remove.

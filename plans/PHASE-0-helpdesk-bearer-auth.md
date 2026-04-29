# Phase 0 — helpdesk-side device-code flow + CLI tokens

This is the helpdesk-side change that **blocks csshd Phase 1**. Lives in
`css-md/CSSHelpdesk`, not in this repo. Captured here so we don't lose it.

## Design — helpdesk brokers the auth

The CLI never talks to Entra. The CLI talks only to the helpdesk. The
helpdesk uses its existing NextAuth+Entra flow to identify the user
(zero new Entra app registrations) and mints its own opaque bearer
tokens that the CLI carries on every API call.

**Why this shape:**
- One Entra app instead of two; no admin consent dance.
- Token revocation is a DB write, not "wait for JWT expiry."
- If we ever swap identity providers, csshd doesn't change.
- No tenant/client IDs leak through `.well-known` (we don't need that
  endpoint at all in this design).
- The CLI's trust boundary is the helpdesk URL, not Microsoft.

## Flow

1. `csshd login --helpdesk https://your-helpdesk-url` →
   `POST /api/v1/cli/auth/init` →
   CLI gets `{ deviceCode, userCode: "ABCD-1234", verificationUri,
   expiresIn: 600, interval: 5 }`.
2. CLI prints:
   `Open https://your-helpdesk-url/cli-link?code=ABCD-1234 and approve.`
3. User opens link in browser. If not signed in, existing NextAuth flow
   bounces them to Entra. Once authenticated, page shows: "Approve CLI
   access for code ABCD-1234? Valid 90 days. Revocable at
   /settings/cli-tokens." Approve / Deny buttons.
4. Approve → `POST /api/v1/cli/auth/approve` → marks session approved,
   links to current `userId`, mints a `CliToken`.
5. CLI is polling `/api/v1/cli/auth/poll` every `interval` seconds. When
   approved, the poll returns `{ accessToken, expiresAt }`. CLI stores
   in OS keychain (via `keyring` crate).
6. Every subsequent helpdesk API call:
   `Authorization: Bearer csshd_<opaque-token>`.

## Schema additions (additive, no `--accept-data-loss` needed)

```prisma
model CliAuthSession {
  id          String   @id @default(cuid())
  deviceCode  String   @unique  // long random — CLI's session handle
  userCode    String   @unique  // short human code shown in browser ("ABCD-1234")
  userId      String?           // null until approved
  user        User?    @relation("CliAuthSessionUser", fields: [userId], references: [id])
  approvedAt  DateTime?
  deniedAt    DateTime?
  expiresAt   DateTime           // ~10 min from create
  createdAt   DateTime @default(now())
  @@index([expiresAt])
}

model CliToken {
  id          String   @id @default(cuid())
  tokenHash   String   @unique  // sha256(token) — never store the token itself
  userId      String
  user        User     @relation("CliTokenUser", fields: [userId], references: [id])
  name        String?            // user-supplied: "Nick's MacBook"
  lastUsedAt  DateTime?
  expiresAt   DateTime            // 90 days from issue
  revokedAt   DateTime?
  createdAt   DateTime @default(now())
  @@index([userId])
}
```

User model gets two back-relations: `cliAuthSessions CliAuthSession[] @relation("CliAuthSessionUser")` and `cliTokens CliToken[] @relation("CliTokenUser")`.

## Endpoints

### Public (no auth required)

- `POST /api/v1/cli/auth/init` — body: `{ name?: string }` (optional client name). Returns:
  ```json
  {
    "deviceCode": "<64-char random>",
    "userCode": "ABCD-1234",
    "verificationUri": "https://your-helpdesk-url/cli-link",
    "verificationUriComplete": "https://your-helpdesk-url/cli-link?code=ABCD-1234",
    "expiresIn": 600,
    "interval": 5
  }
  ```
  Rate limit: 20/min per IP to avoid grinding userCodes.

- `POST /api/v1/cli/auth/poll` — body: `{ deviceCode }`. Returns one of:
  - `200 { accessToken: "csshd_<random>", expiresAt }` — approved, return token (one time only; subsequent polls return 410).
  - `428 { error: "authorization_pending" }` — still waiting.
  - `410 { error: "expired_token" }` — session expired or already consumed.
  - `403 { error: "access_denied" }` — user clicked Deny.
  - `400 { error: "invalid_grant" }` — bad deviceCode.

  CLI side: poll every `interval` seconds (with jitter), stop on terminal status.

### Authenticated (NextAuth session)

- `GET /cli-link?code=ABCD-1234` — Next.js page. Looks up the session by userCode. Shows ticket-number-shaped userCode for confirmation, "Approve" / "Deny" buttons, expiration countdown, current user info ("You're approving access for nrobb@css-md.org").
- `POST /api/v1/cli/auth/approve` — body: `{ userCode, name? }`. Marks session approved + linked to `session.user.id`, mints a token, returns `{ ok: true }`. Does NOT return the token to the browser; only the polling CLI gets it.
- `POST /api/v1/cli/auth/deny` — body: `{ userCode }`. Marks session denied.
- `GET /api/v1/cli/tokens` — list current user's CLI tokens (id, name, lastUsedAt, expiresAt, createdAt).
- `POST /api/v1/cli/tokens/[id]/revoke` — set `revokedAt = now()`.

### Settings page

- `/settings/cli-tokens` — list user's tokens with last-used timestamp + name + revoke button.

## Auth middleware

In `src/lib/auth.ts` (or a wrapper), on each API call:

1. If `Authorization` header starts with `Bearer csshd_`:
   - SHA-256 the full token (`csshd_<random>`).
   - Look up `CliToken` by hash.
   - Reject if `revokedAt != null` or `expiresAt <= now`.
   - Update `lastUsedAt = now()` (debounced — don't write on every request; once per minute is plenty).
   - Attach `{ user: { ... } }` to the request the same shape `auth()` produces today.
2. Otherwise, fall through to the existing NextAuth session cookie path.

## Token format + storage

- `csshd_<32 bytes base64url>` — distinct prefix means we can recognize at a glance and rate-limit unknown prefixes.
- Stored hashed (SHA-256) — even a DB compromise doesn't yield usable tokens.
- 90 day expiry. Auto-rotation can come later; v1 just expires.
- `name` defaults to the user-agent + IP at first use; user can edit on `/settings/cli-tokens`.

## Worker — token cleanup cron

Add a daily worker (`cli-token-cleanup`) that deletes:
- `CliAuthSession` rows past `expiresAt`.
- `CliToken` rows where `revokedAt` is older than 30 days (audit retention).

## Test plan

Unit:
- Token hashing round-trip.
- userCode collision retry (insanely unlikely with ABCD-1234 entropy of ~16M, but guard it).
- Middleware: bearer present → user attached; bearer revoked → 401; bearer expired → 401; no bearer → falls through to NextAuth.

Integration:
- Init → poll (pending) → approve → poll (returns token) → call API with token → returns user data.
- Init → wait → expired.
- Init → deny → poll → 403.
- Token revoked → call API → 401.
- Token name update via /settings/cli-tokens.

## Estimated effort

~half a day:
- 30 min: schema migration + Prisma generate.
- 1 h: init/poll/approve/deny route handlers + rate limit.
- 1 h: bearer middleware in `auth.ts`.
- 1 h: `/cli-link` approval page UI.
- 30 min: `/settings/cli-tokens` management page.
- 30 min: cleanup worker.
- 1 h: tests.

After Phase 0 lands, csshd Phase 1 in this repo can proceed without
further helpdesk-side changes — the API surface for the CLI is just the
existing `/api/v1/tickets/*`, `/api/v1/users/me`, etc., now accepting
Bearer tokens alongside session cookies.

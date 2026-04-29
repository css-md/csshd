# Phase 0 — helpdesk-side bearer-token auth

This is the helpdesk-side change that **blocks csshd Phase 1**. Lives in
`css-md/CSSHelpdesk`, not in this repo. Captured here so we don't lose it.

## What's needed

The helpdesk API today only accepts NextAuth session cookies. The CLI sends
`Authorization: Bearer <jwt>` with an Entra-issued JWT. We need a small
middleware that:

1. Sees `Authorization: Bearer <jwt>` on a request.
2. Validates the JWT:
   - Issuer: `https://login.microsoftonline.com/<tenant-id>/v2.0`. Tenant
     ID lives in `int_csshd_tenant_id` (or env fallback). Not in the
     csshd repo — that's the whole point.
   - Audience: a new "CSS Helpdesk CLI" App Registration's
     Application ID URI (different from the OIDC login app — that one's
     for browser sessions, this one is the API audience the CLI presents).
   - Signature: against MS's JWKS at
     `https://login.microsoftonline.com/<tenant-id>/discovery/v2.0/keys`,
     cached for ~24h.
   - Not expired.
3. Resolves the JWT's `oid` claim (Entra object ID) → `User.entraId` →
   `User`. If found, attaches `{ user: { id, role, ... } }` to the request
   the same shape `auth()` produces today.
4. If the header is absent, falls through to the existing NextAuth path
   (so browser sessions keep working).

## Where to add it

`src/lib/auth-bearer.ts` (new). Then refactor `auth()` in `src/lib/auth.ts`
to call it first; if no Bearer token, fall through to NextAuth's session.

Or — simpler — leave `auth()` alone and add a separate helper
`authOrBearer()` that the API routes opt into. Audit each route handler.
Less invasive but more touch points.

I'd vote for the first approach: `auth()` becomes the unified entry point
that handles both. Single change, single test surface.

## New endpoints

- `GET /.well-known/csshd-config` — **public, unauthenticated** discovery
  endpoint. Returns:
  ```json
  {
    "name": "CSS IT Helpdesk",
    "tenantId": "<from int_csshd_tenant_id>",
    "clientId": "<from int_csshd_client_id>",
    "scope": "<from int_csshd_scope, e.g. api://csshd-cli/Tickets.ReadWrite>",
    "oauthIssuer": "https://login.microsoftonline.com/<tenant>/v2.0",
    "minVersion": "0.1.0"
  }
  ```
  None of these values are secrets — they're OAuth identifiers that have
  to be transmitted in plaintext anyway. They live in SystemConfig
  (`int_csshd_*` keys) so they can be rotated without a binary rebuild
  on the CLI side.

- `POST /api/v1/auth/cli/whoami` — returns the authenticated user's
  profile (id, name, email, role). Used by `csshd whoami`.

(Everything else uses existing `/api/v1/*` routes — those just need to
accept Bearer tokens.)

## Entra app registration

The CLI auths to a *different* Entra app than the website OIDC login.
Need to:

1. Create a new App Registration in the CSS tenant (the existing OIDC app
   is for confidential browser sessions; we want a public client for
   device-code flow).
2. Configure: Allow public client flows = Yes. Redirect URIs: none needed
   for device flow.
3. Expose an API: define a scope `Tickets.ReadWrite` (or similar) that
   the CLI requests.
4. Save Tenant ID + Application (client) ID + Application ID URI into
   helpdesk SystemConfig (`int_csshd_tenant_id`, `int_csshd_client_id`,
   `int_csshd_scope`).

**The csshd repo has no CSS-specific identifiers.** First-run flow:
1. User runs `csshd login --helpdesk https://your-helpdesk-url`
   (or interactive prompt).
2. CLI fetches `<url>/.well-known/csshd-config`.
3. CLI starts device-code flow with the returned tenant/client/scope.
4. URL + config cached in `~/.config/csshd/config.toml`. Subsequent runs
   use the cache; refresh on demand or on auth failure.

The helpdesk side just validates `aud` matches the configured Application
ID URI. No secrets on either side — device code flow doesn't use one.

## Test plan

- Unit test the JWT validator with both valid + tampered tokens, expired
  tokens, wrong issuer, wrong audience.
- Integration test: spin up a mock JWKS, mint a test JWT, hit
  `/api/v1/tickets` with the Bearer header, expect 200.
- Verify cookie auth still works on the same routes (regression).

## Estimated effort

~half a day:
- 30 min: create Entra app registration, document IDs in memory.
- 2 h: bearer middleware, JWKS fetch+cache, validator.
- 1 h: refactor `auth()` to layer bearer in front of NextAuth.
- 1 h: tests.
- 30 min: doc + journal entry.

After Phase 0 lands, Phase 1 in `csshd` can proceed without further
helpdesk-side changes.

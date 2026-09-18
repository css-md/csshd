# Security

## Threat model

`csshd` is a thin client for a helpdesk that issues it a bearer token. It
contains no secrets, no identity-provider configuration, and no CSS-specific
infrastructure detail. Point it at any compatible helpdesk install with
`csshd login --helpdesk <url>`.

The CLI never talks to an identity provider. It talks only to the helpdesk,
which authenticates the user however it likes (Microsoft Entra, in CSS's case)
and mints its own opaque token. That keeps the CLI's trust boundary at a single
URL the user typed.

| Asset | Lives | Defense |
|---|---|---|
| User identity | The helpdesk's IdP | Out of scope for this repo — the CLI never sees it |
| Access token (`csshd_…`) | OS keychain (Keychain / Credential Manager / Secret Service) | Never written to a plaintext file, never logged. Opaque and helpdesk-issued; stored server-side only as a SHA-256 hash |
| Helpdesk URL | `config.toml` in the platform config dir | Not secret — it's the address of a web service |
| Ticket data | Helpdesk server | Server-side authorization; the CLI sees only what the server returns for the bearer token |

There is no refresh token. Tokens expire (90 days, set by the server) and the
user runs `csshd login` again. Revocation is a server-side database write —
sign into the web UI and visit `/settings/cli-tokens`.

## What's intentionally NOT in this repo

- Any client secret, signing key, JWKS or certificate. The device-code flow
  needs none.
- Tenant IDs, client IDs or OAuth scopes. The CLI never contacts the IdP, so it
  never learns them.
- Any IP, internal hostname, or other infrastructure detail of a deployment.

## Reporting

Email `nrobb@css-md.org` with subject `[csshd security]`. Please don't open
public issues for security-sensitive reports.

## Repo controls

- GitHub secret scanning (automatic on public repos).
- TruffleHog runs on every push and pull request, plus a weekly scan over full
  history — see `.github/workflows/security.yml`.
- `cargo audit` runs in CI against the RustSec advisory database.
- Dependabot security updates enabled for cargo and GitHub Actions.
- The release workflow uses `GITHUB_TOKEN` only; no CI secret grants
  production access.

## Token handling rules (for contributors)

- **Never** log token contents, even at debug level.
- **Never** write a token to a file. Use `keyring`. If the keychain is
  unavailable, tell the user to re-auth — a plaintext fallback would be a real
  regression, and `src/credentials.rs` says so where someone would be tempted.
- **Never** let a token reach a panic message or an error chain.
- HTTPS goes through the system trust store via `rustls`. No certificate
  pinning (possible future hardening).
- `csshd` warns on a plaintext `http://` helpdesk URL but does not refuse it —
  a bearer token over cleartext is the user's call to make on a lab instance,
  not ours to silently allow.

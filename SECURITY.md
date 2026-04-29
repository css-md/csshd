# Security

## Threat model

`csshd` is an OAuth 2.0 client. It does not contain or transmit CSS-specific secrets, and the source repo is intentionally generic enough to point at any compatible helpdesk install.

| Asset | Lives | Defense |
|---|---|---|
| User identity | Microsoft Entra | OIDC (out of scope for this repo) |
| Access token | OS keychain (Keychain / Credential Manager / Secret Service) | Never written to plaintext files; never logged |
| Refresh token | OS keychain | Same |
| Helpdesk URL + Tenant/Client IDs | `~/.config/csshd/config.toml` | Not secret — these are OAuth identifiers, equivalent to a username |
| Ticket data | Helpdesk server (gated by Bearer auth) | Server-side authorization; the CLI only sees what the server returns for the authenticated user |

## What's intentionally NOT in this repo

- Tenant ID, Client ID, scope, helpdesk URL — all fetched at runtime from `<helpdesk>/.well-known/csshd-config`.
- Any client secret. Device-code flow uses a public client by design; no secret exists.
- Any signing key, JWKS, or certificate.
- Any IP, internal hostname, or infrastructure detail of the helpdesk deployment.

## Reporting

Email `nrobb@css-md.org` with subject `[csshd security]`. Please don't open public issues for security-sensitive reports.

## Repo controls

- GitHub Secret Scanning enabled (auto-on for public repos).
- Dependabot security updates enabled.
- `gitleaks` configured as a pre-commit hook (see `.pre-commit-config.yaml` if present) and as a CI check on every PR.
- No CI secrets that grant prod access; the release workflow uses `GITHUB_TOKEN` only.

## Token handling rules (for contributors)

- **Never** log token contents, even at debug level. Log `…<redacted>…` or token suffix only.
- **Never** write tokens to a file. Use `keyring`. If the keychain is unavailable, prompt to re-auth — don't fall back to plaintext.
- **Never** include tokens in panic messages or error chains. Strip before propagating.
- All HTTPS requests use the system trust store via `rustls`. No custom cert pinning yet (potential future hardening).

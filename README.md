# csshd

Terminal client for the CSS IT Helpdesk. Triage, claim, comment on, and close tickets from your shell — and run a real TUI when you want to live in it.

> **Status:** Phases 1 and 2 are implemented and the helpdesk side (device-code
> auth) is deployed. A `v0.1.0` tag exists but **no release was ever
> published**, so the installer one-liners below don't work yet — use
> `cargo install --git` or build from source. Re-tagging will produce one.
> See **Roadmap** below for what's left.

## Install

⚠️ **No release has been published yet**, so these two one-liners currently
404. They're what will work once a tag produces a release — until then, use
the Cargo install below.

**Linux / macOS:**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/css-md/csshd/releases/latest/download/csshd-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/css-md/csshd/releases/latest/download/csshd-installer.ps1 | iex"
```

The installers verify the release checksum and refuse to install without it.
They drop the binary in `~/.local/bin` (Linux/macOS) or
`%USERPROFILE%\.csshd\bin` (Windows), and add it to PATH — or tell you how.
Override the location with `CSSHD_INSTALL_DIR`.

**Cargo (if you have Rust installed):**

```sh
cargo install --git https://github.com/css-md/csshd
```

## First run

```sh
csshd login --helpdesk https://your-helpdesk-url     # set the helpdesk + auth
# (subsequent runs remember the URL — no flag needed)
csshd whoami                                          # confirms who you're signed in as
csshd list --mine                                     # shows your open tickets
```

### How auth works

1. The first time you run `csshd login`, you point it at a helpdesk URL.
2. CLI asks the helpdesk for an authorization session. The helpdesk returns a short user-friendly code (e.g. `ABCD-1234`) and a verification URL.
3. CLI prints something like: `Open https://your-helpdesk-url/cli-link?code=ABCD-1234 and approve.`
4. You open the link in your browser. If you're not signed in, the helpdesk's normal SSO flow handles it (Microsoft Entra in CSS's case, but the CLI doesn't care). Once you're authenticated, you see "Approve CLI access for code ABCD-1234?" — click Approve.
5. Meanwhile the CLI is polling. As soon as you approve, the helpdesk hands the CLI an opaque bearer token (`csshd_…`). The token is stored in your **OS keychain** (macOS Keychain / Windows Credential Manager / Linux Secret Service) — never on disk in plaintext.
6. Every subsequent CLI command attaches the token as `Authorization: Bearer csshd_…` on requests to the helpdesk's `/api/v1/*` endpoints.

**The CLI never talks to your identity provider directly.** All identity goes through the helpdesk. That means:

- No tenant IDs, client IDs, or app registrations in this repo.
- Token revocation is a single DB write — log into the web, visit `/settings/cli-tokens`, click Revoke.
- If you ever swap identity providers, the CLI doesn't change.
- A fork pointed at a different helpdesk install needs zero code change.

## Commands (planned)

```
csshd login                     # OIDC device-code login
csshd logout                    # forget stored creds
csshd whoami                    # current user
csshd list [--status open] [--mine] [--assignee=alex]
csshd view CSS-04234            # full ticket view
csshd claim CSS-04234           # assign to me + IN_PROGRESS
csshd close CSS-04234           # set CLOSED
csshd comment CSS-04234 "msg"   # add a public comment
csshd comment CSS-04234 --internal   # opens $EDITOR for an internal note
csshd tui                       # interactive ratatui app (Phase 2)
```

## Roadmap

**Phase 0 (helpdesk-side) — shipped 2026-06-02.**
- Helpdesk-issued bearer tokens. Device-code flow (`/api/v1/cli/auth/init` + `/cli-link` approval page + `/api/v1/cli/auth/poll`), opaque `csshd_…` tokens stored hashed, middleware that accepts them on `/api/v1/*` alongside the existing NextAuth session cookies. Revocation UI at `/settings/cli-tokens`. All of it is live in
`css-md/csshelpdesk`; `plans/PHASE-0-helpdesk-bearer-auth.md` is kept as the
original spec.

**Phase 1 — plumbing CLI — done**, except shell completions:
- ✅ Helpdesk-brokered device-code login (not Entra directly — see Architecture).
- ✅ HTTP client wrapping `/api/v1/*` with typed structs.
- ✅ `list` / `view` / `claim` / `close` / `comment` / `whoami`.
- ✅ Output formats: human (default), `--json` for piping.
- ⬜ Shell completions: `csshd completions bash|zsh|fish|powershell`.

**Phase 2 — real TUI — done**, except live updates:
- ✅ `csshd tui` — ratatui app. Ticket list left, detail right.
- ✅ Vim-style nav: `j/k`/`Enter`/`r`/`/search`/`q`, plus mouse.
- ✅ `$EDITOR` for replies.
- ⬜ Live updates via the helpdesk's existing SSE channel (polls every 30s
  today).

**Phase 3 — polish:**
- Saved views (`csshd view-saved hot`, `csshd view-saved mine-pending`).
- Native desktop notifications on assignment.
- Homebrew tap (`brew install css-md/tap/csshd`).

**Conscious omissions** (use the web UI for these):
- Admin: integrations, system config, support tiers, skill tags.
- Bulk merge / undo workflows — fine in the web, awkward in TUI.
- House Assignments: a different domain model with a print-roster UI.
- Knowledge base / KB authoring — markdown editing in $EDITOR is OK but the web UI is better.
- Reports / charts — terminals don't render bar charts well; pipe to `--json` if you need data.

## Develop

```sh
# Install Rust if you don't have it:
winget install Rustlang.Rustup        # Windows
# or
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # macOS / Linux

cargo build                            # build
cargo run -- login                     # run
cargo test                             # tests
cargo clippy --all-targets             # lint (CI runs this with -D warnings)
cargo fmt --check                      # format

# Cut a release:
git tag v0.1.0 && git push --tags
```

## Architecture

- **Auth:** Helpdesk-brokered device-code flow. The CLI talks only to the helpdesk; the helpdesk handles whatever identity provider it wants behind the scenes. Tokens are opaque `csshd_…` strings stored in OS keychain via [`keyring`](https://crates.io/crates/keyring). Helpdesk-issued, helpdesk-revocable.
- **HTTP:** [`reqwest`](https://crates.io/crates/reqwest) with `rustls-tls` (no OpenSSL dep, simpler cross-compile).
- **TUI:** [`ratatui`](https://ratatui.rs) + [`crossterm`](https://crates.io/crates/crossterm).
- **Distribution:** a hand-rolled `release.yml` cross-compiles for
  `x86_64-{linux,macos,windows}` and `aarch64-macos`, checksums each artifact,
  and publishes via GitHub Releases on git tag. `aarch64-linux` is disabled
  pending a libdbus fix in the `cross` image. (`cargo-dist` was evaluated and
  not adopted; the leftover `[package.metadata.dist]` block in `Cargo.toml` is
  inert.)

## License

MIT — see [LICENSE](./LICENSE).

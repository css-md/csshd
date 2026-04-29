# csshd

Terminal client for the CSS IT Helpdesk. Triage, claim, comment on, and close tickets from your shell — and run a real TUI when you want to live in it.

> ⚠️ **v0.1 scaffold.** Commands currently print "not yet implemented." Phase 1 (real auth + ticket commands) is the next milestone. See **Roadmap** below.

## Install

Once releases ship, these are the one-liners.

**Linux / macOS:**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/css-md/csshd/releases/latest/download/csshd-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/css-md/csshd/releases/latest/download/csshd-installer.ps1 | iex"
```

The installer drops a `csshd` binary in `~/.cargo/bin` (or `%USERPROFILE%\.cargo\bin` on Windows) and tells you to add it to PATH if it isn't already.

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

1. The first time you run `csshd login`, it asks for (or takes via flag) a helpdesk URL.
2. It fetches `<url>/.well-known/csshd-config` — a public, unauthenticated endpoint that returns the OAuth identifiers (tenant ID, client ID, scope) the helpdesk wants the CLI to use.
3. It starts an OAuth 2.0 device authorization flow against Microsoft Entra. You get a code + URL ("Visit https://microsoft.com/devicelogin and enter ABCD-1234"); paste it in your browser, approve, done.
4. The resulting JWT is stored in your **OS keychain** (macOS Keychain / Windows Credential Manager / Linux Secret Service). Never on disk in plaintext.
5. Every subsequent CLI command attaches the JWT as `Authorization: Bearer …` to helpdesk API calls. The helpdesk validates against MS's public JWKS.

**No secrets in this repo.** The `csshd` binary has no CSS-specific identifiers compiled in — it gets them from the helpdesk on first connect. That means:

- A fork of this repo can talk to a different helpdesk install with no code change.
- Rotating the Entra app means updating one row in `SystemConfig` on the helpdesk; CLIs pick it up next launch.
- Anyone reading the source has no extra information to use against you.

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

**Phase 0 (helpdesk-side, blocks Phase 1):**
- Bearer-token middleware on the helpdesk API. Validates Entra-issued JWTs against MS JWKS, maps to User. Today the API only accepts NextAuth session cookies.

**Phase 1 — plumbing CLI (this repo, ~1 week of focused work):**
- OIDC device-code login against Entra; refresh on demand.
- HTTP client wrapping `/api/v1/*` with typed structs.
- `list` / `view` / `claim` / `close` / `comment` / `whoami`.
- Output formats: human (default), `--json` for piping.
- Shell completions: `csshd completions bash|zsh|fish|powershell`.

**Phase 2 — real TUI (~1 week):**
- `csshd tui` — ratatui app. Ticket list left, detail right.
- Vim-style nav: `j/k`/`Enter`/`r`/`/search`/`q`.
- Live updates via the helpdesk's existing SSE channel.
- `$EDITOR` for replies; rich diff for edits.

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
cargo test                             # tests (none yet)
cargo clippy --all-targets             # lint
cargo fmt --check                      # format

# Set up cargo-dist (one-time, unblocks the release pipeline):
cargo install cargo-dist
cargo dist init
git add .github/workflows/release.yml dist-workspace.toml
git commit -m "ci: cargo-dist release pipeline"

# Cut a release:
git tag v0.1.0 && git push --tags
```

## Architecture

- **Auth:** Microsoft Entra OIDC device-code flow. Tokens stored in OS keychain via [`keyring`](https://crates.io/crates/keyring). The CLI talks straight to Entra; the helpdesk validates the resulting JWT.
- **HTTP:** [`reqwest`](https://crates.io/crates/reqwest) with `rustls-tls` (no OpenSSL dep, simpler cross-compile).
- **TUI:** [`ratatui`](https://ratatui.rs) + [`crossterm`](https://crates.io/crates/crossterm).
- **Distribution:** [`cargo-dist`](https://github.com/axodotdev/cargo-dist) cross-compiles for `x86_64-{linux,macos,windows}` and `aarch64-{linux,macos}`, generates installer scripts, and publishes via GitHub Releases on git tag.

## License

MIT — see [LICENSE](./LICENSE).

# csshd — state of the repo, September 2026

Written after a read-through of the whole tree at `023f707`. Last real work
was 2026-04-29 — every commit in the repo lands on that one day, which is
worth remembering when reading the "done" markers below.

## TL;DR

The code is in better shape than the docs. Phases 1 and 2 are actually
implemented (login, whoami, list, view, claim, close, comment, and a working
ratatui app), `cargo check` is clean, and clippy only has 6 cosmetic
warnings. What's rotten is the connective tissue: **zero tests, docs that
describe a design the code abandoned, and a hard dependency on a server-side
change (Phase 0) that this repo has no evidence ever shipped.**

The single most important question before any new feature work:

> **Has csshd ever completed a `login` against the real helpdesk?**

Phase 0 (`plans/PHASE-0-helpdesk-bearer-auth.md`) is the server-side
device-code flow, and the plan says in its own words that it *blocks* Phase 1.
Phase 1 and Phase 2 were both built on 2026-04-29 regardless. If Phase 0 never
landed in `css-md/CSSHelpdesk`, then ~2,100 lines of client code have never
been exercised against a live server and every DTO in `src/client.rs` is a
guess. Answer that first — it changes the priority of everything else on this
list.

## Defects

### Correctness

1. **TUI reply (`r`) runs `$EDITOR` inside the raw-mode alternate screen.**
   `src/tui.rs:465-490` spawns `commands::comment::run(...)`, which calls
   `open_editor()` → `Command::status()`. The comment at `src/tui.rs:470`
   says "we have to take the terminal back to cooked mode first or vim will
   misbehave" — and then doesn't do it. Meanwhile the 250 ms ticker keeps
   redrawing over the editor and the `spawn_keys` blocking task keeps eating
   stdin. This key is effectively broken; it needs a proper
   suspend/restore around the editor call (disable raw mode, leave alt
   screen, run editor, re-enter, force full redraw) and the input task must
   be paused for the duration.

2. **"Refresh after claim" doesn't refresh.** `src/tui.rs:444` sends
   `AppEvent::Tick` with the comment "trigger refresh next tick", but the
   `Tick` handler only refreshes when `last_refresh.elapsed() >= 30s`. Right
   after a claim it hasn't, so nothing happens and the list shows stale state
   for up to 30 seconds. Setting `app.last_refresh = None` is the fix. Same
   gap after close (`x`) from either pane.

3. **`auth_poll` status mapping doesn't match the Phase 0 spec.**
   `src/client.rs:109` treats `408` and `428` as "pending". The spec only
   defines `428`. The spec's `400 invalid_grant` falls through to the generic
   `unexpected status` arm, so a bad device code produces a confusing error
   instead of "this login session is invalid, run login again".

4. **`strip_html` drops block structure.** Both copies (`src/commands/view.rs`
   and `src/tui.rs`) delete tags without translating `<br>`, `</p>`, `</div>`
   or `<li>` into newlines, so `<p>one</p><p>two</p>` renders as `onetwo`.
   Any ticket whose body came from the web editor reads as one run-on
   paragraph. The two copies are also duplicated by hand with a comment
   asking the reader to keep them in sync — pull it into `src/format.rs`.

5. **Comment/code mismatch in `list`.** `src/commands/list.rs:24` says
   "Default to OPEN+IN_PROGRESS if no status filter", but the code passes
   `None` and the server decides. Either implement it or delete the comment;
   right now `csshd list` with no args may well return the whole archive.

6. **`--json` is not actually machine-safe.** Errors print human text to
   stderr and exit 1 even under `--json`, and there's no JSON error envelope.
   Anything scripting against it has to special-case failure.

7. **Token expiry is thrown away.** `TokenResponse.expires_at` is printed once
   at login and never stored, so an expired token surfaces as a bare
   "Unauthorized" rather than "your CLI token expired on <date>, run
   `csshd login`".

### Generalization blockers (these matter for the self-host goal)

8. **`CSS-` is hardcoded.** `src/client.rs:210` and `:217` strip the literal
   prefix and re-pad to exactly 5 digits. Any other install's ticket numbers
   are unresolvable.

9. **Status and priority enums are hardcoded in three places** —
   `src/format.rs`, `src/tui.rs` (`short_status`, `short_priority`,
   `status_color`, `priority_color`). An install with a
   `WAITING_ON_REQUESTER` status renders `????` with no color.

10. **`<!--html-->` body sentinel** is a private convention of CSSHelpdesk,
    baked into both renderers.

11. **CSS-specific schema in the DTOs** — `site`, `team`, `oooStart`/`oooEnd`
    on `WhoAmI`, and the `assignedAgentId` patch shape. Fine for CSS, invisible
    coupling for anyone else.

12. **There is no written API contract.** `/api/v1` is the only versioning,
    and nothing anywhere in either repo pins the request/response shapes.
    This is the real blocker for third-party self-hosting, and it's covered in
    `plans/PHASE-4-itsm-and-self-hosting.md`.

### Docs that are actively wrong

These matter more than usual because the repo is public.

13. **`SECURITY.md` describes a design that was abandoned before Phase 1.**
    It says csshd is "an OAuth 2.0 client", lists a **refresh token** (none
    exists), lists **Tenant/Client IDs in `config.toml`** (none exist), and
    says they're "fetched at runtime from `<helpdesk>/.well-known/csshd-config`"
    — an endpoint that `plans/PHASE-0-helpdesk-bearer-auth.md` explicitly
    decided not to build. It also claims **gitleaks runs as a pre-commit hook
    and a CI check**; there is no `.pre-commit-config.yaml` and CI runs
    TruffleHog. A security policy that misdescribes the security model is
    worse than none.

14. **`README.md` opens with "v0.1 scaffold. Commands currently print 'not yet
    implemented'"** — untrue since 2026-04-29. The Roadmap still lists Phase 1
    and Phase 2 as upcoming, and Phase 1 still says "OIDC device-code login
    against Entra", which the 530994d pivot removed.

15. **`--help` lies.** `src/main.rs:85`: "Open the interactive TUI (Phase 2 —
    not yet implemented)". It is implemented.

16. **Install paths are documented wrong.** README says the installer drops the
    binary in `~/.cargo/bin` / `%USERPROFILE%\.cargo\bin`. `install.sh` uses
    `~/.local/bin`; `install.ps1` uses `%USERPROFILE%\.csshd\bin`. The
    `~/.cargo/bin` claim comes from the unused `[package.metadata.dist]` block.

17. **`completions` is listed in the Phase 1 roadmap but was never built.**

### Build, CI, supply chain

18. **Zero tests.** `cargo test` runs 0 tests; there is no `tests/` directory
    and no `#[cfg(test)]` anywhere. `relative_time`, `strip_html`,
    `resolve_ticket`'s number parsing, and `resolve_helpdesk`'s URL
    normalization are all pure functions sitting right there, and the client
    is a prime candidate for `wiremock`.

19. **CI can't fail on lint.** `cargo fmt --check` and `cargo clippy` both
    carry `continue-on-error: true` (commit 023f707, "stop the failure
    noise"). `cargo fmt --check` currently fails with 39 diff hunks and clippy
    has 6 warnings. The noise won't get smaller on its own.

20. **`install.sh` fails open on checksum verification.** The `.sha256`
    fetch ends in `|| true` and the verify block is skipped entirely if the
    file is missing — a network blip or a stripped release asset silently
    downgrades to an unverified install. It should fail closed.
    (`install.ps1` catches only `System.Net.WebException`, which PowerShell 7
    doesn't throw, so it happens to fail closed there — by accident.)

21. **No dependency audit.** `security.yml` runs TruffleHog for secrets only.
    There's no `cargo audit` / `cargo deny` despite Dependabot being wired up.

22. **`[package.metadata.dist]` is dead config.** The release pipeline is
    hand-rolled in `release.yml`; the README still tells contributors to run
    `cargo dist init`, which would overwrite it. Pick one.

23. **aarch64-linux is disabled** in `release.yml` pending a libdbus fix in the
    `cross` image. Worth revisiting — ARM Linux is a plausible target for a
    self-hosted crowd.

## Suggested order of work

1. Confirm Phase 0 shipped; run `login`/`list`/`view` against the live
   helpdesk and correct the DTOs to match reality.
2. Fix the TUI editor suspend (#1) and the post-action refresh (#2) — both are
   things a daily user hits immediately.
3. Make `SECURITY.md` and `README.md` true. Small, and it's a public repo.
4. Turn on `-D warnings`, fix the 39 fmt hunks and 6 clippy warnings, add
   `cargo audit`, drop `continue-on-error`.
5. Add the first tests — pure functions plus a `wiremock` round-trip for the
   device-code flow.
6. Then, and only then, start `plans/PHASE-4-itsm-and-self-hosting.md`.

Items 1-5 are roughly a week. None of them are interesting, and all of them
get cheaper to do now than after the surface area doubles.

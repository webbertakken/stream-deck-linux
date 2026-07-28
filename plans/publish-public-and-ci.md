# Publish stream-deck-linux public + set up CI

Ship the completed hold-key + native-autostart work to a **public** GitHub repo
and make **CI green** on GitHub. All feature code is already implemented,
committed, and hardware-verified on branch `feat/hold-key-and-autostart`. This
plan is purely about **publishing** and **continuous integration** - do not
touch the feature code except to fix a genuine CI failure.

Tick one checkbox at a time: read the task, do it the best way, verify, tick,
commit (where a commit applies), move on. Never batch ticks.

---

## State of the world (verified, do not redo the scan)

- Repo: `/home/webber/Repositories/stream-deck-linux`, a Rust binary crate
  (`streamdeck`). Local branches:
  - `main` at `9df7ec1` (pre-feature baseline).
  - `feat/hold-key-and-autostart` at `1b9e278` (all the new work: uinput hold
    key, native autostart + dev loop, web-editor control, docs, tests).
- **No git remote exists yet**; the GitHub repo `webbertakken/stream-deck-linux`
  **does not exist**. `Cargo.toml` already points `repository =` at that URL.
- `gh` is authenticated as **webbertakken** (https protocol). Use `gh` for repo
  creation and PR.
- **Sensitivity scan already done and cleared** by the operator's session: no
  secrets/keys/tokens, no personal paths/IPs, no hardcoded device serial, git
  history clean, `.gitignore` excludes `target/` and the local notes
  (`AMBIGUITIES.md`/`DECISIONS.md`/`LESSONS_LEARNED.md`). The only identifying
  data is the author's public git identity in commits, which is expected for a
  public repo. **Do a fast re-confirm (Phase 1), do not re-litigate the verdict.**
- An existing CI workflow is present at `.github/workflows/ci.yml`: fmt (check),
  clippy `-D warnings`, `cargo test --all-targets` on `ubuntu-latest`, triggered
  on push/PR to `main`. It has never run on GitHub (no remote).
- Local quality gate is green: 116 tests, clippy clean, fmt clean. The uinput
  real-device test is gated to skip when `/dev/uinput` is not writable, and the
  `hold_probe` example / hardware checks are not part of `cargo test`.

## Decisions baked in (implement as stated)

- **Public** repository (operator confirmed).
- Repo name/owner: `webbertakken/stream-deck-linux` (matches `Cargo.toml`).
- Base branch is `main`; the feature lands via a **PR** from
  `feat/hold-key-and-autostart` into `main` so CI runs on the PR.
- CI must be **green on GitHub** before the PR is considered done. If a CI-only
  failure appears (e.g. a test that assumes hardware, a `/dev/uinput` assumption,
  a platform difference on the runner), fix the **root cause** - gate/skip
  hardware-dependent tests properly, never weaken an assertion or delete a test.
- Use HTTPS remotes (matches the authed `gh` protocol).

## Ambiguities to surface to the operator (do NOT guess)

- **A1**: repository **description** and **topics** text - propose sensible
  defaults (description from `Cargo.toml`; topics like `stream-deck`, `elgato`,
  `linux`, `rust`, `hidraw`, `uinput`) and proceed; only escalate if unsure.
- **A2**: anything irreversible beyond "create public repo + push + PR" (e.g.
  enabling branch protection, adding collaborators, deleting/rewriting history,
  force-pushing) - escalate before doing it.
- **A3**: if CI reveals a failure that can only be fixed by changing feature
  behaviour (not just test gating/config), escalate with the specifics before
  changing behaviour.

---

## Phase 1 - Pre-flight re-confirmation (fast, no code changes)

- [ ] `git status` clean on `feat/hold-key-and-autostart`; the plans files and
      any intended docs are committed. Nothing unexpected untracked that should
      be published (the local notes stay gitignored).
- [ ] Fast secret re-scan: `git grep -nIiE "api[_-]?key|secret|token|password|
      private[_-]?key|-----BEGIN"` over tracked files excluding `Cargo.lock` -
      confirm only benign matches. Do not deep-dive; the verdict is settled.
- [ ] Confirm `.gitignore` still excludes `target/`, `AMBIGUITIES.md`,
      `DECISIONS.md`, `LESSONS_LEARNED.md`, and that none of those are tracked
      (`git ls-files | grep -E 'AMBIGUITIES|DECISIONS|LESSONS'` returns nothing).
- [ ] Confirm `main` builds/tests are green locally on the branch tip
      (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test`), so we publish from a known-green state.

## Phase 2 - Create the public GitHub repo + remote

- [ ] Create the repo without pushing yet:
      `gh repo create webbertakken/stream-deck-linux --public
      --description "<desc>" --source . --remote origin` (or create then
      `git remote add origin https://github.com/webbertakken/stream-deck-linux.git`).
      Pick the flag combination that creates an **empty** public repo and wires
      `origin`, without auto-pushing an unintended branch. Verify with
      `git remote -v` and `gh repo view --json visibility,name,owner`.
- [ ] Confirm visibility is **public** (`gh repo view --json visibility`).
- [ ] Set repository description and topics (A1 defaults). Verify.

## Phase 3 - Push base and feature branches

- [ ] Push `main` first so the PR has a base:
      `git push -u origin main`. Verify the branch appears on GitHub and that
      GitHub set `main` as the default branch (set it if not).
- [ ] Push the feature branch:
      `git push -u origin feat/hold-key-and-autostart`. Verify.
- [ ] Confirm no other local branches leak (only `main` +
      `feat/hold-key-and-autostart` should be on the remote).

## Phase 4 - CI: make it run and go green on GitHub

- [ ] Confirm the push triggered the `CI` workflow (push to `main` and/or the
      upcoming PR). List runs: `gh run list --limit 5`.
- [ ] Watch the run to completion: `gh run watch <run-id>` (or poll
      `gh run view <run-id>`). Capture the outcome of each job step (fmt,
      clippy, test).
- [ ] If any step fails, diagnose from the logs (`gh run view <run-id> --log
      --job <job-id>`) and fix the **root cause**:
      - Hardware/`/dev/uinput`-dependent tests MUST be skipped cleanly on the
        runner (verify the gating actually holds on GitHub's `ubuntu-latest`,
        where `/dev/uinput` is typically absent). If a test still tries to open
        the device, fix the gate - do not delete the test.
      - `cargo test --all-targets` also builds examples (`hold_probe`,
        `preview`, `gen-icon`); ensure they **compile** on the runner. Fix
        compile issues; do not remove examples.
      - Any clippy/fmt drift: fix the code, re-run locally, re-push.
      - Commit each fix on the feature branch with a concise message; every push
        re-triggers CI - verify locally first to avoid burning CI minutes.
- [ ] Iterate until the `CI` workflow is **green** on GitHub for the feature
      branch / PR. Record the passing run URL.
- [ ] Add a CI status badge to the top of `README.md`
      (`![CI](https://github.com/webbertakken/stream-deck-linux/actions/
      workflows/ci.yml/badge.svg)`), commit, push, and confirm the badge
      resolves to the green run. (Adjust the workflow `name`/path in the badge
      URL to match `ci.yml`.)

## Phase 5 - Open the pull request

- [ ] Open the PR from `feat/hold-key-and-autostart` into `main`:
      `gh pr create --base main --head feat/hold-key-and-autostart --title
      "<=52 char title" --body "<concise bulleted body>"`. Body covers: native
      autostart + dev loop replacing PM2; the `hold = "<key|combo>"` uinput
      latch (safety: kernel releases on fd close); web-editor control; tests
      116; hardware-verified. No wiki/PM2 cross-links, British English, no
      em-dash.
- [ ] Confirm CI runs on the PR and is green (`gh pr checks`). If red, loop back
      to Phase 4.
- [ ] Verify the PR shows the expected diff (feature commits only, no stray
      files) and the committed tree matches intent (`gh pr diff --name-only`).

## Phase 6 - Verify + report

- [ ] Final confirmation: repo is public, `main` is default, CI green on the PR,
      badge live, PR open and mergeable.
- [ ] Update `~/PR_RESULTS.md` (append, max 8 lines): repo published public, CI
      wired and green, PR opened - with the PR URL and passing run URL.
- [ ] Do NOT merge the PR unless the operator asks; leave it open for review.
- [ ] Report back with: repo URL, PR URL, CI run URL, and anything the operator
      must action (e.g. branch protection if desired - that is A2, ask first).

---

## Definition of done

- `webbertakken/stream-deck-linux` exists as a **public** repo with `main` as
  default branch and the feature branch pushed.
- The `CI` workflow runs on GitHub and is **green** (fmt, clippy `-D warnings`,
  tests), with hardware-dependent tests correctly skipped on the runner and all
  examples compiling.
- A CI badge is live in `README.md`.
- A PR from `feat/hold-key-and-autostart` into `main` is open, green, and shows
  the intended diff.
- No secrets published; nothing irreversible done without operator sign-off.
- Every checkbox ticked as its task landed; a commit per code-changing task.

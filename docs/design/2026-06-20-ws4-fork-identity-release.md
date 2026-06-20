# WS4 — Fork identity, CI & release independence

Status: PLAN · 2026-06-20 · fork: tekgnosis-net/ht32-panel

## Goal

Make the fork a self-sufficient independent project: keep the name **ht32-panel**, own
README/branding, own **signed apt/dnf repo** + GitHub Releases, own **cachix** cache, and go
**daemon-focused** (retire the desktop applet). Releases must include all fork features (WS1
resilience, WS2 Layout, WS3 headless packaging).

## Decisions (locked)

- Name: keep `ht32-panel` (own README/distribution; org `tekgnosis-net` distinguishes it).
- Releases: own signed apt/dnf repo on `tekgnosis-net.github.io/ht32-panel` + GitHub Releases.
- CI cache: own cachix cache (replace `ananthb`).
- Applet: retired — drop the GUI tray applet + AppImage + GTK build path; ship daemon + CLI.
- Version: bump `0.8.1 → 0.9.0` (first independent fork release).
- crates.io publish: **dropped** (crate names owned upstream; cannot republish).

---

## Part A — "I do" (code/config, no account access needed)

### A1. Metadata rebrand
- `Cargo.toml` (workspace.package): `authors = ["tekgnosis-net <kumar@tekgnosis.net>"]`,
  `repository = "https://github.com/tekgnosis-net/ht32-panel"`, `version = "0.9.0"`.
- `packaging/nfpm.yaml` + `nfpm-gui.yaml`(deleted, see A3): `maintainer`/`homepage` → fork.
- `flake.nix`: `homepage = "https://github.com/tekgnosis-net/ht32-panel"`.

### A2. README rewrite
Keep the name; reframe as the independent headless-first continuation (web-GUI management; WS1
self-heal + WS2 Layout engine highlights; applet retired). Repoint every URL: badges →
`tekgnosis-net/.../actions`, screenshots → `raw.githubusercontent.com/tekgnosis-net/...`,
install/download → `github.com/tekgnosis-net/ht32-panel/releases` +
`tekgnosis-net.github.io/ht32-panel`, `nix run github:tekgnosis-net/ht32-panel`.

### A3. Retire the applet (drop the GTK path)
- Delete crate `crates/ht32-panel-applet/`; remove it from the workspace `Cargo.toml` members.
- Delete `packaging/nfpm-gui.yaml` and `packaging/org.ht32panel.Daemon.desktop` usage.
- `flake.nix`: remove the `release-appimage` build, the applet package, and the applet from the
  `release` bundle / `libDeps` GTK inputs; keep the daemon/CLI release tarball.
- `release.yml`: remove the applet nfpm build steps + the `.desktop` staging.

### A4. Drop the crates.io publish job
- `release.yml`: delete the entire `publish:` job (all 5 `cargo publish` steps). The fork ships
  via the signed repo + GitHub Releases, not crates.io.

### A5. Consolidate WS3 (headless packaging) onto `main`
- Bring `feat/headless-packaging`'s packaging onto `main`: `packaging/org.ht32panel.Daemon.conf`,
  `packaging/postinstall.sh`, the `nfpm.yaml` dbus-policy + `scripts.postinstall`, and the
  `release.yml` staging of those two files. (The PR branch stays untouched as the upstream offer.)
- Cherry-pick `e89d244` onto `main`, then reconcile with the A1/A3/A4 `release.yml`/`nfpm.yaml` edits.

### A6. Workflow cache + repo pointers (needs B-values to finalize)
- `ci.yml`, `release.yml`, `docs.yml`: cachix `name: ananthb` → `name: <YOUR_CACHIX_CACHE>`.
- `packaging/ht32-panel.sources` (apt): `URIs: https://tekgnosis-net.github.io/ht32-panel/deb/`.
- `packaging/ht32-panel.repo` (dnf): `baseurl`/`gpgkey` → `https://tekgnosis-net.github.io/ht32-panel/rpm/`.
- `packaging/GPG-KEY` + `packaging/ht32-panel.gpg`: replace with the fork's **public** key (from B2).

---

## Part B — "You do" (GitHub/account/keys; I can't)

### B1. Cachix cache
- Create a cache at cachix.org (e.g. name `tekgnosis` or `ht32-panel-tekgnosis`).
- Repo → Settings → Secrets → Actions → add `CACHIX_AUTH_TOKEN` (a write token: `cachix authtoken`).
- **Tell me the exact cache name** so I can wire it into the 3 workflows (A6).

### B2. GPG signing key (for the apt/dnf repo)
```bash
gpg --full-generate-key            # RSA 4096, no expiry or long expiry; name it "ht32-panel (tekgnosis-net)"
gpg --armor --export <KEYID>             > GPG-KEY          # PUBLIC key -> give me to commit (A6)
gpg --armor --export-secret-keys <KEYID> > private.asc      # PRIVATE key -> next line
```
- Repo → Settings → Secrets → Actions → add `GPG_PRIVATE_KEY` = contents of `private.asc`.
- **Give me the `GPG-KEY` (public) contents** to commit into `packaging/`. Keep `private.asc` secret; delete the local copy after adding the secret.

### B3. GitHub Pages
- Repo → Settings → Pages → Source = **GitHub Actions** (so `docs.yml` can deploy the repo+docs site).

### B4. Nothing needed for
- cosign artifact signing (keyless OIDC — works as-is).
- crates.io (publish job dropped).

---

## Sequencing & validation

1. I do A1–A5 now (rebrand, README, applet retirement, drop publish, WS3 consolidation) — none need B.
2. You do B1–B3 in parallel; send me the cachix cache name (B1) + the public `GPG-KEY` (B2).
3. I finalize A6 with those values.
4. Cut the first fork release: tag `v0.9.0` → `release.yml` builds+signs deb/rpm + GitHub Release;
   push to `main` → `docs.yml` regenerates the signed repo on Pages. Verify `apt install` from the
   fork's repo on a test box (or pve3) end-to-end.

## Notes
- This is config/infra, not feature code — no TDD; verification is `nix flake check` / a dry
  `nfpm package` locally where possible, and a real `v0.9.0` release as the end-to-end test.
- WS2 (Phase 1b+) continues independently on `main`; WS4 doesn't block it.

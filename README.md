# Turfrace

Turfrace is a fast local multiplayer territory game for 1–8 humans, with NPCs filling a configurable 2–12 competitor field. A single human can race the bots in a full-screen view. Draw loops, steal turf, and cut exposed trails; the first competitor to control 95% of the irregular paper arena wins.

The v0.1 implementation is a static Rust/Bevy 0.19 WebAssembly application. It has no server, telemetry, or network play. Profiles, preferences, lobby choices, and lifetime statistics remain in browser-local storage. Fresh lobbies start with no robots; add them explicitly with the robot stepper.

Territory is exact fixed-point vector geometry with a bounded render/broadphase cache. Every
competitor has one spawn-anchored island: a capture severs any other lobe, and a cube standing on
that removed lobe is displaced through the normal respawn flow.

## Run it

Native development:

```sh
cargo run
```

Web development requires the WASM target and [Trunk-rs](https://github.com/trunk-rs/trunk):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
trunk serve --open
```

To test the optimized browser build locally, let Trunk build the release WASM
and serve the generated static site:

```sh
trunk serve --release --open
```

This serves the same release output that is deployed, with live reload enabled.
For a deploy-style build followed by a separate static server, run:

```sh
./scripts/build-web
python3 -m http.server 8080 --directory dist
```

Create the optimized static site with:

```sh
./scripts/build-web
```

The helper enforces a release Trunk build, rejects debug-sized WASM, and records gzip (and Brotli when installed) sizes. The deployable files are written to `dist/`. Serve them from HTTPS for reliable browser Gamepad API access. WebGL2 is the baseline renderer; the custom paper, territory, and trail shaders do not require WebGPU.

## Controls

- Gamepad: either stick steers, A/Cross readies or unreadies in the lobby, B/Circle leaves, D-pad/left stick navigates, shoulders cycle lobby colors, and Start readies in the lobby or pauses in play.
- Mouse: click `JOIN WITH MOUSE`, then click the pulsing ready prompt; click it again during the shared countdown to cancel.
- Keyboard: Enter/Space joins or toggles ready; WASD/arrow keys join when unassigned, then steer and navigate, and Escape pauses or goes back.

The race countdown begins automatically as soon as every joined racer is ready. There is no separate Start press; any racer can unready before launch to cancel the countdown.

If a controller disconnects during play, the match pauses. An unassigned controller can reclaim the player, or the paused player can be replaced with an NPC.

## Quality checks

The repository's normal handoff check formats, compiles, lints, and runs every test:

```sh
./scripts/feedback
```

Use `./scripts/feedback --quick` while iterating. The test suite covers geometry, capture and combat ordering, respawn and victory boundaries, persistence/input/lobby behavior, viewport layouts, render mappings, and an accelerated deterministic one-hour NPC soak.

NPCs use bounded local sensing and a deterministic tactical capture planner. Focused tests cover
plan sizing, risk response, action commitment, and realized multi-NPC capture quality; the
accelerated soak separately checks long-running authoritative invariants.

## Controlled visual playtests

The visual harness boots the real game plugins at a fixed timestep and captures deterministic user-facing frames:

```sh
./scripts/visual-feedback home
./scripts/visual-feedback leaderboard
./scripts/visual-feedback lobby-empty
./scripts/visual-feedback lobby-robots
./scripts/visual-feedback countdown
./scripts/visual-feedback match --frames 180
./scripts/visual-feedback match --seconds 6
./scripts/visual-feedback capture --seconds 6
./scripts/visual-feedback respawn
./scripts/visual-feedback disconnect
./scripts/visual-feedback game-over
./scripts/visual-feedback results --width 960 --height 600
./scripts/visual-feedback settings
./scripts/visual-feedback --all
./scripts/ui-preview lobby-cards
./scripts/ui-preview hud
```

Images are written to `target/visual-feedback/`. Frame and second offsets use the game's fixed 60 Hz clock, while optional dimensions make layout stress tests repeatable. The script uses Xvfb and Mesa's software Vulkan driver by default so people or automated review agents can inspect actual rendering in headless Linux. `VK_ICD_FILENAMES`, `WGPU_BACKEND`, and `WGPU_SETTINGS_PRIO` remain overridable for another test environment.

For fast territory-only iteration, the predefined-shape review tool exercises the production smooth
vector-surface renderer without starting a match:

```sh
./scripts/territory-review target/visual-feedback/territory-review.png
```

## Architecture

`src/main.rs` only composes three top-level plugins:

- `AppShellPlugin`: states, profiles/persistence, input abstraction, lobby, responsive UI, and browser integration.
- `MatchPlugin`: the deterministic 60 Hz authoritative board, movement, trails, capture, combat, ranking, respawn, and NPC simulation.
- `PresentationPlugin`: split-screen cameras, revision-built elevated territory surfaces, lean flat-shaded cube/trail/effect rendering, and a shared CC0 arcade sound suite.

Gameplay is 2D and deterministic even though presentation is 3D. Input and NPCs both produce the same steering intent, and presentation consumes simulation events without owning rules. Territory ownership is converted into bounded smooth owner meshes only when its revision changes; the grid is never rendered directly and no territory work runs on ordinary frames.

## CI and GitHub Pages deployment

[`.github/workflows/ci-pages.yml`](.github/workflows/ci-pages.yml) runs formatting, compilation,
Clippy, and tests for every pull request and push. After a successful `main` build, it creates the
release WASM site with the correct GitHub Pages base path and deploys `dist/`. A manual run is also
available from **Actions → CI and Pages → Run workflow**.

One-time repository setup:

1. In **Settings → Pages → Build and deployment**, set **Source** to **GitHub Actions**.
2. Ensure Actions are enabled under **Settings → Actions → General**. The workflow uses only the
   automatically provided `GITHUB_TOKEN`; no repository secrets or deploy keys are required.
3. Optionally protect the automatically created `github-pages` environment under
   **Settings → Environments** so only `main` can deploy.

Without a custom domain, the site is published at
`https://<owner>.github.io/<repository>/` (for this repository,
`https://toonsevrin.github.io/turfrace/`). GitHub may require Pages to be enabled on a public
repository depending on the account plan.

### Custom domain with Cloudflare DNS

Configure the custom domain in **GitHub Settings → Pages** *before* publishing its DNS records;
GitHub also recommends verifying the domain under the owner account's **Settings → Pages** to
prevent takeover. Then add one of these records in Cloudflare DNS:

- **Subdomain** (recommended), such as `play.example.com`: add a `CNAME` named `play` targeting
  `<owner>.github.io` (not `/turfrace` and not the custom domain).
- **Apex domain**, such as `example.com`: add four `A` records named `@`, targeting
  `185.199.108.153`, `185.199.109.153`, `185.199.110.153`, and `185.199.111.153`. Cloudflare's
  apex CNAME flattening can alternatively point `@` to `<owner>.github.io`.

Start with the records set to **DNS only** (gray cloud), remove any conflicting `A`, `AAAA`, or
`CNAME` records, and do not use wildcard DNS records. Configure both apex and `www` if both should
work; GitHub redirects one to the domain selected in Pages settings. DNS and certificate issuance
can take up to 24 hours. Once GitHub shows the domain check as successful, enable **Enforce HTTPS**.
Cloudflare proxying can be enabled afterward if desired; retain GitHub's origin HTTPS and use
Cloudflare SSL/TLS mode **Full (strict)**.

See GitHub's [custom-domain instructions](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site)
and Cloudflare's [CNAME flattening documentation](https://developers.cloudflare.com/dns/cname-flattening/).

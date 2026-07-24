# Turfrace design notes

These are intentional product-level guardrails for future UI work.

## Menu language

- Menu controls, cards, panels, and focus treatments use crisp square geometry. Do not add
  rounded corners, pill buttons, or radius-based containers.
- Keep the paper field, ink typography, colored rails, and sparse deliberate rules as the visual
  vocabulary. Do not use repeated row underlines as a substitute for hierarchy or spacing.
- Decorative turf marks may remain organic/circular because they are background texture, not UI
  containers.
- Prefer integrated text, color, motion, and icon cues over opaque HUD slabs or diagnostic copy.
- Keep copy lean. Let hierarchy, color, symbols, spacing, and motion carry meaning before adding
  another label or sentence.

## Gameplay information

- Player names belong above the world cubes in their player color.
- The live HUD should stay quiet: top-right leaderboard and short event announcements are enough;
  do not duplicate rank, percentage, or speed readouts in every split-screen viewport.
- Leaderboards should read as a compact color signal: square accent rails, player-color rows, and
  minimal titling are preferred over a wordy panel.
- Speed and danger should be communicated through movement, camera, trail, and effect treatment
  rather than numeric telemetry.

## Fast visual checks

Run `scripts/scoreboard-preview 4` to capture the live top-right leaderboard as an isolated crop
at `target/visual-feedback/scoreboard.png` before changing its spacing, color, or typography.

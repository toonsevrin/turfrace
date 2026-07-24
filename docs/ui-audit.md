# UI audit

This log records the deterministic review passes behind the current shell. Re-run the named
scenario at 1280×720 and, where noted, 960×600 before changing its hierarchy.

## Home

1. Removed the prose slogan competing with the primary action.
2. Split navigation from a large field silhouette to improve first-glance hierarchy.
3. Replaced decorative bubbles with turf regions that teach the game's visual language.
4. Added three passive racers and attached trails so motion communicates play without copy.
5. Stress-checked title/menu clearance against the arena at compact landscape sizes.

## Racer selection

1. Replaced the debug-form layout with two strongly colored player cards.
2. Promoted profile, color, and readiness; demoted device and CPU metadata.
3. Kept pattern customization as a compact labeled selector without adding a redundant second
   identity badge to the card.
4. Removed duplicate P1/P2 labels and replaced text device names with drawn device silhouettes.
5. Reduced each card to profile, color, and pattern selectors, a new-profile action, and one
   explicit ready/unready action; changed the color strip to a proportional swatch and added a
   reduced-motion-aware pulse behind unready device prompts; verified the two-column 960×600
   composition.
6. Replaced the ambiguous total-racers/CPU sentence with an add-robots stepper showing only a
   robot icon and count; fresh two-player lobbies show 0 and the old capacity shows 10.

## Local leaderboard

1. Added column headings so values no longer require inference.
2. Added best-turf percentage, the statistic most closely tied to the game's goal.
3. Replaced free-floating color text with consistent player-color rails.
4. Gave first place a quiet tint while preserving scan speed for the remaining rows.
5. Checked the empty state and four-profile populated state at deterministic ordering.

## Live match HUD

1. Isolated the top-right crop at early and late deterministic match times.
2. Added claimed percentage to explain rank movement.
3. Increased type size and row spacing for split-screen viewing distance.
4. Replaced detached square swatches with thin color rails.
5. Moved the kill feed closer to the ranking block and checked it against active gameplay noise.

## Pause and controller loss

1. Increased scrim strength so the frozen match reads as context rather than UI noise.
2. Preserved Resume as the dominant action and reduced restart/return emphasis.
3. Added overlay-specific text colors instead of reusing paper-screen utility colors.
4. Replaced controller-specific takeover prose with the correct universal input instruction.
5. Captured normal and disconnected variants over the same deterministic match.

## Results and game over

1. Promoted the winner above the statistics table.
2. Renamed terse columns around player intent: Turf and Best Loop.
3. Highlighted the winning row with a quiet player-color tint.
4. Preserved all eight competitors and four actions in the 960×600 stress capture.
5. Simplified game-over copy to one winner name, one field-claimed cue, and one action.

## Settings

1. Split controls into two balanced functional columns.
2. Added section rails and restrained column surfaces for chunking without heavy panels.
3. Kept values aligned and colored while leaving labels neutral.
4. Kept destructive profile/data controls visually secondary until focused or confirmed.
5. Verified all profile rows and the Back action remain visible at the standard capture size.

## Countdown, respawn, and loading

1. Captured countdown independently from active play.
2. Increased respawn hierarchy and separated the label from the timer.
3. Applied the same treatment to the temporary shield state.
4. Added a compact three-color arena-building meter to loading.
5. Added dedicated countdown and respawn scenarios to prevent regressions in these transient states.

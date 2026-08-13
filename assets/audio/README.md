# Turfrace sound suite

The shipped sounds are curated from Kenney's CC0 packs:

- [Interface Sounds](https://kenney.nl/assets/interface-sounds) — menu navigation, confirmation, join, ready, pause, and resume.
- [Digital Audio](https://kenney.nl/assets/digital-audio) — countdown, launch, trail, capture, kill, respawn, leader, and victory cues.
- [Impact Sounds](https://kenney.nl/assets/impact-sounds) — collision and death cues.
- [Head in the Sand](https://opengameart.org/content/head-in-the-sand-seamless-loop) by
  congusbongus — subtle looping background music, released under CC0.

These assets permit commercial use without attribution. The Kenney license text is kept
beside the effects in `Kenney_*_License.txt`; retain it if assets are redistributed.

`src/audio/mod.rs` is the auditable cue manifest. Most cues ship with two carefully
selected variations and alternate them deterministically to prevent repetitive playback.
Frequently repeated menu, capture, and trail-cut cues deliberately use the packs' shortest
clean one-shots; longer multi-stage effects become muddy and can expose Web Audio/Vorbis tail
artifacts when several cues overlap. All effects are intentionally non-spatial because Turfrace
is a shared-screen game.

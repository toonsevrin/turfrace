//! Compact, stable orientation cues for each local player's viewport.
use super::*;
use crate::trail::ActiveTrail;

#[derive(Component)]
pub(super) struct LocalRacerHint(Entity);

pub(super) fn spawn_identity(
    root: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    subject: Entity,
    competitor: &Competitor,
    scale: f32,
) {
    root.spawn((Node {
        position_type: PositionType::Absolute,
        top: px(12),
        left: px(12),
        max_width: percent(48),
        flex_direction: FlexDirection::Column,
        row_gap: px(4),
        ..default()
    },))
        .with_children(|badge| {
            badge.spawn((
                Text::new(competitor.display_name.to_uppercase()),
                TextFont {
                    font: theme.display_font.clone(),
                    font_size: FontSize::Px(12.0 * scale),
                    ..default()
                },
                TextColor(INK),
                TextLayout::justify(Justify::Left),
            ));
            badge.spawn((
                Text::new("HOME / SAFE"),
                TextFont {
                    font: theme.body_font.clone(),
                    font_size: FontSize::Px(10.0 * scale),
                    ..default()
                },
                TextColor(MUTED),
                LocalRacerHint(subject),
            ));
        });
}

pub(super) fn update_local_hints(
    racers: Query<(&LifeState, Option<&ActiveTrail>)>,
    mut hints: Query<(&LocalRacerHint, &mut Text)>,
    mut respawns: Query<(&HumanRespawnText, &mut Visibility)>,
) {
    for (marker, mut text) in &mut hints {
        let next = racers
            .get(marker.0)
            .map_or("", |(life, trail)| hint(life.is_alive(), trail.is_some()));
        if text.0 != next {
            text.0.clear();
            text.0.push_str(next);
        }
    }
    for (marker, mut visibility) in &mut respawns {
        let next = respawn_visibility(racers.get(marker.0).is_ok_and(|(life, _)| !life.is_alive()));
        if *visibility != next {
            *visibility = next;
        }
    }
}

fn respawn_visibility(dead: bool) -> Visibility {
    if dead {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

fn hint(alive: bool, exposed: bool) -> &'static str {
    match (alive, exposed) {
        (false, _) => "BACK IN A MOMENT",
        (true, true) => "TRAIL OUT / LOOP HOME",
        (true, false) => "HOME / SAFE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn respawn_overlay_only_shows_for_dead_racers() {
        assert_eq!(respawn_visibility(true), Visibility::Inherited);
        assert_eq!(respawn_visibility(false), Visibility::Hidden);
    }

    #[test]
    fn trail_cue_explains_the_action_and_never_calls_a_dead_racer_safe() {
        assert_eq!(hint(true, false), "HOME / SAFE");
        assert_eq!(hint(true, true), "TRAIL OUT / LOOP HOME");
        assert_eq!(hint(false, false), "BACK IN A MOMENT");
        assert_eq!(hint(false, true), "BACK IN A MOMENT");
    }
}

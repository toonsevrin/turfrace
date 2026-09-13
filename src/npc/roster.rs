use crate::{board::DeterministicRng, ids::MAX_COMPETITORS};

use super::*;

const NAMES: [&str; 32] = [
    "BIX", "MOKO", "ZAP", "TILLY", "CRUMB", "NOVA", "PIP", "RUNE", "JUNO", "KIP", "DOT", "WOBBLE",
    "MISO", "QUILL", "TANGO", "FIZZ", "NIM", "ROOK", "POGO", "LUMA", "BOLT", "YUKI", "PEBBLE",
    "VEX", "MINT", "ORI", "SCOOT", "ECHO", "DASH", "BEE", "MARMOT", "INK",
];

#[derive(Clone, Debug, PartialEq)]
pub struct NpcRosterEntry {
    pub name: String,
    pub profile: NpcProfile,
}

/// Policy is selected from authored choices first; competence is sampled
/// separately from difficulty bounds. This prevents hard NPCs becoming a
/// uniformly aggressive role and keeps seeded profiles replayable.
pub fn generate_npc_roster(
    seed: u64,
    count: usize,
    difficulty: NpcDifficulty,
) -> Vec<NpcRosterEntry> {
    let count = count.min(MAX_COMPETITORS);
    let mut rng = DeterministicRng::new(seed);
    let mut names = NAMES;
    for index in (1..names.len()).rev() {
        names.swap(index, rng.index(index + 1));
    }
    (0..count)
        .map(|slot| {
            let policy = generate_policy(&mut rng, slot);
            let competence = generate_competence(&mut rng, difficulty);
            NpcRosterEntry {
                name: names[slot].to_owned(),
                profile: NpcProfile { policy, competence },
            }
        })
        .collect()
}

fn generate_policy(rng: &mut DeterministicRng, slot: usize) -> NpcPolicy {
    // Every role remains represented in a full roster, while subsequent
    // choices are authored variations independent of competence.
    match slot % 3 {
        0 => NpcPolicy::Builder(BuilderPolicy {
            shape: match rng.index(4) {
                0 => BuilderShape::Fill,
                1 => BuilderShape::Seal,
                2 => BuilderShape::BroadSweep,
                _ => BuilderShape::Roamer,
            },
            side: if rng.unit_f32() < 0.5 {
                TurnSide::Left
            } else {
                TurnSide::Right
            },
        }),
        1 => NpcPolicy::Hunter(HunterPolicy {
            target: match rng.index(3) {
                0 => HunterTarget::Trail,
                1 => HunterTarget::ExposedRival,
                _ => HunterTarget::Opportunistic,
            },
        }),
        _ => NpcPolicy::Raider(RaiderPolicy {
            objective: match rng.index(3) {
                0 => RaidObjective::Leader,
                1 => RaidObjective::WeakestBorder,
                _ => RaidObjective::TrailCut,
            },
            shape: if rng.unit_f32() < 0.5 {
                RaidShape::Hook
            } else {
                RaidShape::Wedge
            },
        }),
    }
}
fn generate_competence(rng: &mut DeterministicRng, difficulty: NpcDifficulty) -> NpcCompetence {
    let (minimum, maximum) = difficulty.competence_bounds();
    NpcCompetence {
        skill: triangular(rng.unit_f32(), minimum, maximum, (minimum + maximum) * 0.5),
    }
}
pub fn triangular(unit: f32, minimum: f32, maximum: f32, mode: f32) -> f32 {
    let unit = unit.clamp(0.0, 1.0);
    if (maximum - minimum).abs() < f32::EPSILON {
        return minimum;
    }
    let split = (mode - minimum) / (maximum - minimum);
    if unit < split {
        minimum + (unit * (maximum - minimum) * (mode - minimum)).sqrt()
    } else {
        maximum - ((1.0 - unit) * (maximum - minimum) * (maximum - mode)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn roster_is_deterministic_and_names_are_unique() {
        let a = generate_npc_roster(42, 12, NpcDifficulty::Normal);
        let b = generate_npc_roster(42, 12, NpcDifficulty::Normal);
        assert_eq!(a, b);
        assert_eq!(
            a.iter()
                .map(|x| x.name.as_str())
                .collect::<HashSet<_>>()
                .len(),
            a.len()
        );
    }
    #[test]
    fn authored_roles_are_mixed_at_every_difficulty() {
        for d in NpcDifficulty::ALL {
            let roster = generate_npc_roster(77, 8, d);
            assert!(
                roster
                    .iter()
                    .any(|x| matches!(x.profile.policy, NpcPolicy::Builder(_)))
            );
            assert!(
                roster
                    .iter()
                    .any(|x| matches!(x.profile.policy, NpcPolicy::Hunter(_)))
            );
            assert!(
                roster
                    .iter()
                    .any(|x| matches!(x.profile.policy, NpcPolicy::Raider(_)))
            );
        }
    }
    #[test]
    fn competence_is_bounded_and_seeded_independently_of_role() {
        for d in NpcDifficulty::ALL {
            let (min, max) = d.competence_bounds();
            for entry in generate_npc_roster(9, 12, d) {
                let skill = entry.profile.competence.skill;
                assert!((min..=max).contains(&skill));
            }
        }
    }
    #[test]
    fn triangular_clamps_input() {
        assert!((triangular(-1.0, 0.2, 0.8, 0.5) - 0.2).abs() < 1e-5);
        assert!((triangular(2.0, 0.2, 0.8, 0.5) - 0.8).abs() < 1e-5);
    }
}

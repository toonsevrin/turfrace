use crate::board::DeterministicRng;

use super::{NpcBrainKind, NpcDifficulty, NpcTraits};

const NAMES: [&str; 32] = [
    "BIX", "MOKO", "ZAP", "TILLY", "CRUMB", "NOVA", "PIP", "RUNE", "JUNO", "KIP", "DOT", "WOBBLE",
    "MISO", "QUILL", "TANGO", "FIZZ", "NIM", "ROOK", "POGO", "LUMA", "BOLT", "YUKI", "PEBBLE",
    "VEX", "MINT", "ORI", "SCOOT", "ECHO", "DASH", "BEE", "MARMOT", "INK",
];

#[derive(Clone, Debug, PartialEq)]
pub struct NpcRosterEntry {
    pub name: String,
    pub brain_kind: NpcBrainKind,
    pub traits: NpcTraits,
}

pub fn generate_npc_roster(
    seed: u64,
    count: usize,
    difficulty: NpcDifficulty,
) -> Vec<NpcRosterEntry> {
    let count = count.min(12);
    let mut rng = DeterministicRng::new(seed);
    let mut names = NAMES;
    for index in (1..names.len()).rev() {
        let other = rng.index(index + 1);
        names.swap(index, other);
    }
    let legacy_slot = (rng.unit_f32() < 0.03).then(|| rng.index(count.max(1)));
    (0..count)
        .map(|slot| NpcRosterEntry {
            name: names[slot].to_owned(),
            brain_kind: if legacy_slot == Some(slot) {
                NpcBrainKind::LegacyWanderer
            } else {
                NpcBrainKind::Tactical
            },
            traits: generate_traits(&mut rng, difficulty),
        })
        .collect()
}

fn generate_traits(rng: &mut DeterministicRng, difficulty: NpcDifficulty) -> NpcTraits {
    let (minimum, maximum, mode) = difficulty.skill_bounds();
    let skill = triangular(rng.unit_f32(), minimum, maximum, mode);
    let adapts = rng.unit_f32() >= 0.20;
    NpcTraits {
        skill,
        aggression: blended_trait(rng, 0.25 + skill * 0.30),
        greed: blended_trait(rng, 0.45),
        exploration: blended_trait(rng, 0.38).max(0.08),
        composure: blended_trait(rng, 0.25 + skill * 0.45),
        adaptability: if adapts {
            (rng.unit_f32() * (0.35 + skill * 0.65)).max(0.03)
        } else {
            0.0
        },
        commitment: blended_trait(rng, 0.42),
        turning_bias: rng.range_f32(-1.0, 1.0),
    }
}

fn blended_trait(rng: &mut DeterministicRng, center: f32) -> f32 {
    (rng.unit_f32() * 0.72 + center * 0.28).clamp(0.0, 1.0)
}

pub fn triangular(unit: f32, minimum: f32, maximum: f32, mode: f32) -> f32 {
    let unit = unit.clamp(0.0, 1.0);
    let split = (mode - minimum) / (maximum - minimum);
    if unit < split {
        minimum + (unit * (maximum - minimum) * (mode - minimum)).sqrt()
    } else {
        maximum - ((1.0 - unit) * (maximum - minimum) * (maximum - mode)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn roster_is_deterministic_and_names_are_unique() {
        let first = generate_npc_roster(42, 12, NpcDifficulty::Normal);
        let second = generate_npc_roster(42, 12, NpcDifficulty::Normal);
        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<HashSet<_>>()
                .len(),
            first.len()
        );
    }

    #[test]
    fn roster_caps_legacy_brains_at_one() {
        for seed in 0..2_000 {
            let roster = generate_npc_roster(seed, 12, NpcDifficulty::Normal);
            assert!(
                roster
                    .iter()
                    .filter(|entry| entry.brain_kind == NpcBrainKind::LegacyWanderer)
                    .count()
                    <= 1
            );
        }
    }

    #[test]
    fn legacy_match_probability_is_three_percent() {
        let legacy_matches = (0..10_000)
            .filter(|seed| {
                generate_npc_roster(*seed, 8, NpcDifficulty::Normal)
                    .iter()
                    .any(|entry| entry.brain_kind == NpcBrainKind::LegacyWanderer)
            })
            .count();
        assert!((260..=340).contains(&legacy_matches), "{legacy_matches}");
    }

    #[test]
    fn difficulty_skill_distributions_have_exact_bounds_and_overlap() {
        for difficulty in NpcDifficulty::ALL {
            let (minimum, maximum, _) = difficulty.skill_bounds();
            let skills: Vec<_> = (0..2_000)
                .flat_map(|seed| generate_npc_roster(seed, 1, difficulty))
                .map(|entry| entry.traits.skill)
                .collect();
            assert!(
                skills
                    .iter()
                    .all(|skill| *skill >= minimum && *skill <= maximum)
            );
        }
        assert!(NpcDifficulty::Easy.skill_bounds().1 > NpcDifficulty::Normal.skill_bounds().0);
        assert!(NpcDifficulty::Normal.skill_bounds().1 > NpcDifficulty::Hard.skill_bounds().0);
    }

    #[test]
    fn twenty_percent_of_generated_bots_do_not_adapt() {
        let non_adaptive = (0..10_000)
            .flat_map(|seed| generate_npc_roster(seed, 1, NpcDifficulty::Normal))
            .filter(|entry| entry.traits.adaptability == 0.0)
            .count();
        assert!((1_850..=2_150).contains(&non_adaptive), "{non_adaptive}");
    }
}

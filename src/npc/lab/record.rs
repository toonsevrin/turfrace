use crate::{match_game::SimulationEvent, territory_map::TerritoryMap};

use super::model::{EncounterFixture, RecordedEvent};

pub(crate) fn event_name(event: &SimulationEvent) -> &'static str {
    match event {
        crate::match_game::SimulationEvent::Countdown(_) => "countdown",
        crate::match_game::SimulationEvent::Go => "go",
        crate::match_game::SimulationEvent::TrailStarted { .. } => "trail-started",
        crate::match_game::SimulationEvent::Capture { .. } => "capture",
        crate::match_game::SimulationEvent::Death { .. } => "death",
        crate::match_game::SimulationEvent::Kill { .. } => "kill",
        crate::match_game::SimulationEvent::Respawn { .. } => "respawn",
        crate::match_game::SimulationEvent::RankingChanged => "ranking-changed",
        crate::match_game::SimulationEvent::Victory { .. } => "victory",
    }
}

pub(crate) fn record_event(event: &SimulationEvent) -> RecordedEvent {
    match event {
        crate::match_game::SimulationEvent::Countdown(value) => {
            RecordedEvent::Countdown { value: *value }
        }
        crate::match_game::SimulationEvent::Go => RecordedEvent::Go,
        crate::match_game::SimulationEvent::TrailStarted { player } => {
            RecordedEvent::TrailStarted { player: player.0 }
        }
        crate::match_game::SimulationEvent::Capture {
            player,
            area,
            stolen_area,
            loop_fill,
        } => RecordedEvent::Capture {
            player: player.0,
            area: *area,
            stolen_area: *stolen_area,
            loop_fill: *loop_fill,
        },
        crate::match_game::SimulationEvent::Death {
            victim,
            killer,
            cause,
        } => RecordedEvent::Death {
            victim: victim.0,
            killer: killer.map(|id| id.0),
            cause: format!("{cause:?}"),
        },
        crate::match_game::SimulationEvent::Kill { killer, progress } => RecordedEvent::Kill {
            killer: killer.0,
            total: progress.total,
            streak: progress.streak,
        },
        crate::match_game::SimulationEvent::Respawn { player } => {
            RecordedEvent::Respawn { player: player.0 }
        }
        crate::match_game::SimulationEvent::RankingChanged => RecordedEvent::RankingChanged,
        crate::match_game::SimulationEvent::Victory { winner } => {
            RecordedEvent::Victory { winner: winner.0 }
        }
    }
}

pub(crate) fn setup_hash(
    fixture: EncounterFixture,
    field: u64,
    npc: u64,
    board: u64,
    map: &TerritoryMap,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for word in [
        fixture as u64,
        field,
        npc,
        board,
        map.revision(),
        map.arena_area().to_bits() as u64,
    ] {
        hash ^= word;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    for geometry in std::iter::once(map.arena()).chain(map.territories()) {
        for contour in geometry.contours() {
            // Area alone is not an identity: distinct contours can have the
            // same area and otherwise permit replay/setup false positives.
            hash ^= contour.len() as u64;
            hash = hash.wrapping_mul(0x1000_0000_01b3);
            for point in contour {
                hash ^= point.x as u32 as u64;
                hash = hash.wrapping_mul(0x1000_0000_01b3);
                hash ^= point.y as u32 as u64;
                hash = hash.wrapping_mul(0x1000_0000_01b3);
            }
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

pub(super) fn tactic_kind(kind: crate::npc::NpcTacticKind) -> &'static str {
    match kind {
        crate::npc::NpcTacticKind::Return(_) => "return",
        crate::npc::NpcTacticKind::Capture(_) => "capture",
        crate::npc::NpcTacticKind::Hunt(_) => "hunt",
        crate::npc::NpcTacticKind::Raid(_) => "raid",
        crate::npc::NpcTacticKind::Roam => "roam",
    }
}

pub(super) fn tactic_detail(kind: crate::npc::NpcTacticKind) -> Option<String> {
    use crate::npc::{CapturePurpose, HuntTarget, RaidTarget, ReturnReason};
    match kind {
        crate::npc::NpcTacticKind::Return(reason) => Some(
            match reason {
                ReturnReason::Safety => "safety",
                ReturnReason::TrailLimit => "trail-limit",
                ReturnReason::Threat => "threat",
                ReturnReason::CaptureComplete => "capture-complete",
                ReturnReason::Recovery => "recovery",
            }
            .to_owned(),
        ),
        crate::npc::NpcTacticKind::Capture(purpose) => Some(
            match purpose {
                CapturePurpose::FillFrontier => "fill-frontier",
                CapturePurpose::SealGap => "seal-gap",
                CapturePurpose::CutTrail => "cut-trail",
                CapturePurpose::RaidBorder => "raid-border",
            }
            .to_owned(),
        ),
        crate::npc::NpcTacticKind::Hunt(target) => Some(match target {
            HuntTarget::Segment { owner, segment } => format!("segment:{}:{segment}", owner.0),
            HuntTarget::Rival(owner) => format!("rival:{}", owner.0),
        }),
        crate::npc::NpcTacticKind::Raid(target) => Some(match target {
            RaidTarget::Border { owner, .. } => format!("border:{}", owner.0),
            RaidTarget::Leader(owner) => format!("leader:{}", owner.0),
        }),
        crate::npc::NpcTacticKind::Roam => None,
    }
}

pub(super) fn tactic_phase(phase: crate::npc::TacticPhase) -> &'static str {
    match phase {
        crate::npc::TacticPhase::Acquiring => "acquiring",
        crate::npc::TacticPhase::Travelling => "travelling",
        crate::npc::TacticPhase::Committing => "committing",
        crate::npc::TacticPhase::Aborting => "aborting",
    }
}

pub(super) fn tactic_label(kind: crate::npc::NpcTacticKind) -> String {
    match (tactic_kind(kind), tactic_detail(kind)) {
        (kind, Some(detail)) => format!("{kind}({detail})"),
        (kind, None) => kind.to_owned(),
    }
}

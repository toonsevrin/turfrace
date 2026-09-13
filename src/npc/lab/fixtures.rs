use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    geometry::MultiPolygon,
    ids::CompetitorId,
    match_game::{
        Competitor, LastOwnedCell, MatchPhase, MatchSession, MatchSpec, MatchStatistics,
        PresentationReady, RosterDescriptor, SpawnProtection, SteeringCommand, TerritoryRecord,
    },
    movement::CompetitorMotion,
    territory_map::TerritoryMap,
    trail::{ActiveTrail, update_trail_raster},
};

use super::{
    model::{EncounterFixture, LabError},
    record::setup_hash,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct FixtureSetup {
    pub(super) setup_hash: u64,
    pub(super) board_generation: u64,
}

/// Applies an authored policy while preserving production-generated skill.
pub(super) fn apply_profile(world: &mut World, profile: super::model::PersonalityVariant) {
    if profile == super::model::PersonalityVariant::Baseline {
        return;
    }
    let mut query = world.query::<&mut crate::npc::NpcController>();
    for mut controller in query.iter_mut(world) {
        controller.profile.policy = match profile {
            super::model::PersonalityVariant::Builder => {
                crate::npc::NpcPolicy::Builder(crate::npc::BuilderPolicy {
                    shape: crate::npc::BuilderShape::Fill,
                    side: crate::npc::TurnSide::Left,
                })
            }
            super::model::PersonalityVariant::Hunter => {
                crate::npc::NpcPolicy::Hunter(crate::npc::HunterPolicy {
                    target: crate::npc::HunterTarget::Trail,
                })
            }
            super::model::PersonalityVariant::Raider => {
                crate::npc::NpcPolicy::Raider(crate::npc::RaiderPolicy {
                    objective: crate::npc::RaidObjective::Leader,
                    shape: crate::npc::RaidShape::Hook,
                })
            }
            super::model::PersonalityVariant::Baseline => unreachable!(),
        };
    }
}

pub(super) fn roster_for(fixture: EncounterFixture) -> Vec<RosterDescriptor> {
    let human = |id: u8| RosterDescriptor::Human {
        identity: format!("lab-human-{id}"),
        display_name: format!("Lab Human {id}"),
        color_id: id,
        pattern_id: id % 8,
    };
    match fixture {
        EncounterFixture::ReturnRace
        | EncounterFixture::Interception
        | EncounterFixture::LoopCapture
        | EncounterFixture::BridgeCapture => vec![RosterDescriptor::Npc, human(1)],
        EncounterFixture::BaitDisengagement => vec![RosterDescriptor::Npc, human(1), human(2)],
        EncounterFixture::DistractedTerritory => vec![
            RosterDescriptor::Npc,
            human(1),
            human(2),
            human(3),
            human(4),
        ],
    }
}

fn point(name: &str, point: Vec2, map: &TerritoryMap, margin: f32) -> Result<Vec2, LabError> {
    if !point.is_finite() || map.arena_signed_distance(point) < margin {
        return Err(LabError::InvalidSetup(format!(
            "fixture point {name} is outside generated arena: {point:?}"
        )));
    }
    Ok(point)
}

fn rectangle(center: Vec2, half: Vec2) -> MultiPolygon {
    MultiPolygon::from_outer(&[
        center - half,
        Vec2::new(center.x + half.x, center.y - half.y),
        center + half,
        Vec2::new(center.x - half.x, center.y + half.y),
    ])
}

pub(super) fn install_fixture(
    world: &mut World,
    fixture: EncounterFixture,
    spec: &MatchSpec,
) -> Result<FixtureSetup, LabError> {
    let center = Vec2::ZERO;
    let positions = [
        point(
            "npc_home",
            center + Vec2::new(-12.0, 0.0),
            world.resource::<TerritoryMap>(),
            3.0,
        )?,
        point(
            "outbound_exit",
            center + Vec2::new(-5.0, 0.0),
            world.resource::<TerritoryMap>(),
            1.0,
        )?,
        point(
            "return_exit",
            center + Vec2::new(-12.0, 5.0),
            world.resource::<TerritoryMap>(),
            1.0,
        )?,
        point(
            "intercept_crossing",
            center + Vec2::new(-1.0, 0.0),
            world.resource::<TerritoryMap>(),
            1.0,
        )?,
    ];
    let mut claims = vec![
        (CompetitorId(0), rectangle(positions[0], Vec2::splat(3.5))),
        (
            CompetitorId(1),
            rectangle(Vec2::new(12.0, 0.0), Vec2::splat(3.5)),
        ),
    ];
    if matches!(fixture, EncounterFixture::BaitDisengagement) {
        claims.push((
            CompetitorId(2),
            rectangle(Vec2::new(0.0, 13.0), Vec2::splat(2.5)),
        ));
    }
    if matches!(fixture, EncounterFixture::DistractedTerritory) {
        claims[1].1 = rectangle(Vec2::new(12.0, 0.0), Vec2::new(7.0, 5.0));
        for id in 2..5 {
            claims.push((
                CompetitorId(id),
                rectangle(Vec2::new(0.0, -14.0 + id as f32), Vec2::splat(2.0)),
            ));
        }
    }
    if matches!(fixture, EncounterFixture::LoopCapture) {
        claims.push((
            CompetitorId(0),
            rectangle(Vec2::new(-12.0, 8.0), Vec2::splat(2.5)),
        ));
    }
    if matches!(fixture, EncounterFixture::BridgeCapture) {
        claims.push((
            CompetitorId(0),
            rectangle(Vec2::new(12.0, 10.0), Vec2::splat(2.5)),
        ));
    }
    {
        let board = world.resource::<BoardGrid>().clone();
        let map = {
            let mut territory = world.resource_mut::<TerritoryMap>();
            for id in 0..spec.roster.len() {
                territory.clear_owner(CompetitorId(id as u8));
            }
            for (owner, claim) in claims {
                if claim.bounds().is_none() {
                    return Err(LabError::InvalidSetup(format!(
                        "empty claim for owner {owner:?}"
                    )));
                }
                territory.apply_claim(owner, claim);
            }
            territory.clone()
        };
        let mut query = world.query::<(
            &Competitor,
            &mut CompetitorMotion,
            &mut LastOwnedCell,
            &mut TerritoryRecord,
            &mut MatchStatistics,
            &mut SpawnProtection,
        )>();
        for (competitor, mut motion, mut last_owned, mut record, mut stats, mut protection) in
            query.iter_mut(world)
        {
            let target = if competitor.id == CompetitorId(0) {
                positions[0]
            } else if competitor.id == CompetitorId(1) {
                if matches!(fixture, EncounterFixture::Interception) {
                    Vec2::new(8.0, 0.0)
                } else {
                    Vec2::new(12.0, 0.0)
                }
            } else if matches!(fixture, EncounterFixture::DistractedTerritory) {
                Vec2::new(0.0, -14.0 + competitor.id.0 as f32)
            } else {
                Vec2::new(0.0, 13.0)
            };
            let target = point("competitor_spawn", target, &map, 1.0)?;
            let heading = match fixture {
                EncounterFixture::ReturnRace | EncounterFixture::LoopCapture => Vec2::X,
                EncounterFixture::Interception | EncounterFixture::BridgeCapture => Vec2::Y,
                _ => (Vec2::ZERO - target).normalize_or(Vec2::Y),
            };
            *motion = CompetitorMotion::new(target, heading);
            let last_owned_position = if competitor.id == CompetitorId(1)
                && matches!(fixture, EncounterFixture::Interception)
            {
                Vec2::new(10.0, 0.0)
            } else {
                target
            };
            last_owned.0 = board.world_to_cell(last_owned_position).ok_or_else(|| {
                LabError::InvalidSetup("fixture position has no board cell".into())
            })?;
            record.current_area = map.area(competitor.id);
            record.peak_area = record.current_area;
            stats.peak_territory_area = record.current_area;
            *protection = SpawnProtection {
                remaining: 0.0,
                elapsed: 1.0,
            };
        }
        if matches!(fixture, EncounterFixture::Interception) {
            let owner = CompetitorId(1);
            let boundary = Vec2::new(8.5, 0.0);
            let start_cell = board.world_to_cell(Vec2::new(10.0, 0.0)).ok_or_else(|| {
                LabError::InvalidSetup("interception trail has no start cell".into())
            })?;
            let mut trail = ActiveTrail::new(owner, start_cell, boundary, -Vec2::X);
            trail.append_exact(Vec2::new(8.0, 0.0));
            {
                let mut board_resource = world.resource_mut::<BoardGrid>();
                update_trail_raster(&mut board_resource, &mut trail, spec.config.trail_width);
            }
            let entity = {
                let mut query = world.query::<(Entity, &Competitor)>();
                query
                    .iter(world)
                    .find(|(_, competitor)| competitor.id == owner)
                    .map(|(entity, _)| entity)
            }
            .ok_or_else(|| LabError::InvalidSetup("interception actor entity is missing".into()))?;
            world.entity_mut(entity).insert(trail);
        }
        let mut rankings = world.resource_mut::<crate::match_game::Rankings>();
        rankings.entries.clear();
        let mut ids = (0..spec.roster.len())
            .map(|id| CompetitorId(id as u8))
            .collect::<Vec<_>>();
        ids.sort_by(|a, b| map.area(*b).total_cmp(&map.area(*a)).then_with(|| a.cmp(b)));
        rankings
            .entries
            .extend(ids.into_iter().enumerate().map(|(index, id)| {
                crate::match_game::RankingEntry {
                    id,
                    rank: (index + 1) as u8,
                    territory_area: map.area(id),
                    territory_percent: map.area_percent(id),
                    alive: true,
                    kills: 0,
                }
            }));
        {
            let mut session = world.resource_mut::<MatchSession>();
            session.phase = MatchPhase::Running;
            session.elapsed_seconds = 0.0;
            session.countdown_ticks_remaining = 0;
            session.winner = None;
        }
        let generation = world.resource::<crate::match_game::MatchGeneration>().0;
        world.resource_mut::<PresentationReady>().0 = Some(generation);
    }
    let board_generation = world.resource::<BoardGrid>().generation_revision;
    let map = world.resource::<TerritoryMap>();
    let setup_hash = setup_hash(
        fixture,
        spec.seed,
        spec.npc_roster_seed,
        board_generation,
        map,
    );
    Ok(FixtureSetup {
        setup_hash,
        board_generation,
    })
}

pub(super) fn npc_ids(world: &mut World) -> Vec<u8> {
    let mut query = world.query::<(&Competitor, Option<&crate::npc::NpcController>)>();
    let mut result = query
        .iter(world)
        .filter_map(|(c, npc)| npc.map(|_| c.id.0))
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

pub(super) fn puppet_commands(
    fixture: EncounterFixture,
    tick: u64,
    spec: &MatchSpec,
) -> Vec<SteeringCommand> {
    let t = tick as f32 / spec.config.fixed_hz as f32;
    let direction = |player: u8, direction: Vec2| {
        SteeringCommand::new(
            tick,
            CompetitorId(player),
            direction.normalize_or(Vec2::Y),
            1.0,
        )
    };
    match fixture {
        EncounterFixture::ReturnRace => {
            vec![direction(1, if t < 3.0 { -Vec2::X } else { Vec2::X })]
        }
        EncounterFixture::Interception => {
            vec![direction(1, if t < 4.0 { -Vec2::X } else { Vec2::Y })]
        }
        EncounterFixture::BaitDisengagement => vec![direction(1, Vec2::X), direction(2, -Vec2::X)],
        EncounterFixture::DistractedTerritory => vec![direction(1, Vec2::X)],
        EncounterFixture::LoopCapture | EncounterFixture::BridgeCapture => {
            vec![direction(1, Vec2::Y)]
        }
    }
}

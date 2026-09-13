use bevy::prelude::Vec2;

use super::model::{
    EncounterFixture, LabAcceptance, LabAcceptanceCheck, LabManeuverStats, LabTrace, RecordedEvent,
    ReplayTick,
};

fn acceptance_check(
    name: &str,
    passed: bool,
    requirement: &str,
    observed: impl Into<String>,
) -> LabAcceptanceCheck {
    LabAcceptanceCheck {
        name: name.to_owned(),
        passed,
        requirement: requirement.to_owned(),
        observed: observed.into(),
    }
}

fn first_event_tick(
    expected: &[ReplayTick],
    matches: impl Fn(&RecordedEvent) -> bool,
) -> Option<u64> {
    expected.iter().find_map(|tick| {
        tick.events
            .iter()
            .any(&matches)
            .then_some(tick.snapshot.tick)
    })
}

fn first_event_tick_after(
    expected: &[ReplayTick],
    after: u64,
    matches: impl Fn(&RecordedEvent) -> bool,
) -> Option<u64> {
    expected.iter().find_map(|tick| {
        (tick.snapshot.tick >= after && tick.events.iter().any(&matches))
            .then_some(tick.snapshot.tick)
    })
}

fn event_tick_at(
    expected: &[ReplayTick],
    tick: u64,
    matches: impl Fn(&RecordedEvent) -> bool,
) -> Option<u64> {
    expected.iter().find_map(|candidate| {
        (candidate.snapshot.tick == tick && candidate.events.iter().any(&matches)).then_some(tick)
    })
}

fn npc_death_ticks_in(
    expected: &[ReplayTick],
    npc: u8,
    start: u64,
    end: u64,
) -> Vec<(u64, String)> {
    expected
        .iter()
        .filter(|tick| tick.snapshot.tick >= start && tick.snapshot.tick <= end)
        .flat_map(|tick| {
            tick.events.iter().filter_map(|event| match event {
                RecordedEvent::Death { victim, cause, .. } if *victim == npc => {
                    Some((tick.snapshot.tick, cause.clone()))
                }
                _ => None,
            })
        })
        .collect()
}

fn npc_tactic_seen(trace: &LabTrace, kind: &str, detail: Option<&str>, start: u64) -> Option<u64> {
    trace
        .decisions
        .iter()
        .filter(|decision| decision.npc == 0 && decision.tick >= start)
        .find_map(|decision| {
            (decision.tactic_kind.as_deref() == Some(kind)
                && detail.is_none_or(|detail| decision.tactic_detail.as_deref() == Some(detail)))
            .then_some(decision.tick)
        })
}

fn first_outbound_tick(trace: &LabTrace) -> Option<u64> {
    trace
        .decisions
        .iter()
        .filter(|decision| decision.npc == 0)
        .find_map(|decision| {
            matches!(
                decision.tactic_kind.as_deref(),
                Some("capture" | "hunt" | "raid")
            )
            .then_some(decision.tick)
        })
}

fn first_abandonment_tick(trace: &LabTrace, start: u64, end: u64) -> Option<u64> {
    trace
        .decisions
        .iter()
        .filter(|decision| decision.npc == 0 && decision.tick >= start && decision.tick <= end)
        .find_map(|decision| {
            (decision.tactic_transition
                && matches!(decision.tactic_kind.as_deref(), Some("return"))
                && matches!(
                    decision.previous_tactic_kind.as_deref(),
                    Some("capture" | "hunt" | "raid")
                ))
            .then_some(decision.tick)
        })
}

fn trajectory_window(trace: &LabTrace, npc: u8, start: u64, end: u64) -> (f32, usize, bool) {
    let Some(points) = trace.trajectories.get(&npc) else {
        return (0.0, 0, false);
    };
    let Some(ticks) = trace.trajectory_ticks.get(&npc) else {
        return (0.0, 0, false);
    };
    let Some(ownership) = trace.trajectory_ownership.get(&npc) else {
        return (0.0, 0, false);
    };
    let count = points.len().min(ticks.len()).min(ownership.len());
    let mut travelled = 0.0;
    let mut exposed = 0;
    let mut left_owned = false;
    let mut reentered = false;
    for index in 0..count {
        if ticks[index] < start || ticks[index] > end {
            continue;
        }
        if !ownership[index] {
            exposed += 1;
            left_owned = true;
        } else if left_owned {
            reentered = true;
        }
        if index > 0 && ticks[index - 1] >= start && ticks[index] <= end && ticks[index - 1] <= end
        {
            travelled +=
                Vec2::from_array(points[index]).distance(Vec2::from_array(points[index - 1]));
        }
    }
    (travelled, exposed, reentered)
}

fn first_reentry_tick(trace: &LabTrace, npc: u8, start: u64) -> Option<u64> {
    let ownership = trace.trajectory_ownership.get(&npc)?;
    let ticks = trace.trajectory_ticks.get(&npc)?;
    let count = ownership.len().min(ticks.len());
    let mut left_owned = false;
    for index in 0..count {
        if ticks[index] < start {
            continue;
        }
        if ownership[index] {
            if left_owned {
                return Some(ticks[index]);
            }
        } else {
            left_owned = true;
        }
    }
    None
}

fn first_positive_capture(
    expected: &[ReplayTick],
    player: u8,
    loop_fill: bool,
    start: u64,
) -> Option<u64> {
    first_event_tick_after(
        expected,
        start,
        |event| matches!(event, RecordedEvent::Capture { player: owner, area, loop_fill: fill, .. } if *owner == player && *fill == loop_fill && *area > 0.0),
    )
}

fn first_positive_theft(expected: &[ReplayTick], player: u8, start: u64) -> Option<u64> {
    first_event_tick_after(
        expected,
        start,
        |event| matches!(event, RecordedEvent::Capture { player: owner, stolen_area, .. } if *owner == player && *stolen_area > 0.0),
    )
}

fn first_return_terminal(
    expected: &[ReplayTick],
    trace: &LabTrace,
    return_tick: Option<u64>,
    run_end: u64,
) -> u64 {
    let first_reentry = return_tick.and_then(|tick| first_reentry_tick(trace, 0, tick));
    let first_death = return_tick.and_then(|tick| {
        first_event_tick_after(
            expected,
            tick,
            |event| matches!(event, RecordedEvent::Death { victim, .. } if *victim == 0),
        )
    });
    [first_reentry, first_death]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(run_end)
}

fn territory_changed_at(expected: &[ReplayTick], tick: u64) -> bool {
    if tick < 2 {
        return false;
    }
    expected
        .get(tick as usize - 2)
        .zip(expected.get(tick as usize - 1))
        .is_some_and(|(before, after)| {
            before.snapshot.territory_fingerprint != after.snapshot.territory_fingerprint
        })
}

pub(crate) fn evaluate_acceptance(
    fixture: EncounterFixture,
    expected: &[ReplayTick],
    trace: &LabTrace,
    stats: &LabManeuverStats,
) -> LabAcceptance {
    let mut checks = Vec::new();
    let run_end = expected.last().map_or(0, |tick| tick.snapshot.tick);
    match fixture {
        EncounterFixture::ReturnRace => {
            let maneuver_start = first_outbound_tick(trace).unwrap_or(1);
            let return_tick = npc_tactic_seen(trace, "return", None, maneuver_start);
            let terminal = first_return_terminal(expected, trace, return_tick, run_end);
            let (_, _, reentered) = trajectory_window(trace, 0, maneuver_start, terminal);
            let npc_deaths =
                npc_death_ticks_in(expected, 0, return_tick.unwrap_or(maneuver_start), terminal);
            let returns = return_tick.is_some();
            let abandoned = first_abandonment_tick(trace, maneuver_start, terminal).is_some();
            checks.push(acceptance_check(
                "return-tactic",
                returns,
                "NPC must apply a return tactic during the exposed encounter",
                format!("first return tactic tick={return_tick:?}"),
            ));
            checks.push(acceptance_check(
                "tactic-abandonment",
                abandoned,
                "an outbound tactic must transition to the production safety-return state",
                format!("outbound-to-safety-return transition={abandoned}"),
            ));
            checks.push(acceptance_check(
                "owned-reentry",
                reentered,
                "NPC trajectory must leave owned ground and later re-enter it",
                format!("trajectory ownership re-entry={reentered}"),
            ));
            checks.push(acceptance_check(
                "survives-return",
                npc_deaths.is_empty(),
                "NPC must not emit a Death event in the return window",
                format!("NPC death events={npc_deaths:?}"),
            ));
        }
        EncounterFixture::Interception => {
            // This fixture installs and rasterizes an already exposed trail.
            // It therefore has no TrailStarted event until a later life.
            let initial_trail_tick = expected.first().and_then(|tick| {
                tick.snapshot
                    .competitors
                    .iter()
                    .any(|competitor| competitor.id == 1 && competitor.trail_length > 0.0)
                    .then_some(tick.snapshot.tick)
            });
            let trail_tick = initial_trail_tick.or_else(|| {
                first_event_tick(
                    expected,
                    |event| matches!(event, RecordedEvent::TrailStarted { player } if *player == 1),
                )
            });
            let target_terminal = trail_tick.and_then(|start| {
                first_event_tick_after(expected, start, |event| match event {
                    RecordedEvent::Death { victim, .. } => *victim == 1,
                    RecordedEvent::Capture { player, .. } => *player == 1,
                    _ => false,
                })
            });
            let cut_tick = trail_tick.and_then(|start| {
                first_event_tick_after(expected, start, |event| {
                    matches!(event, RecordedEvent::Death { victim, killer: Some(killer), cause } if *victim == 1 && *killer == 0 && cause == "TrailCut")
                })
            });
            let cut_tick = cut_tick.filter(|cut| Some(*cut) == target_terminal);
            let kill_tick = cut_tick.and_then(|cut| {
                event_tick_at(
                    expected,
                    cut,
                    |event| matches!(event, RecordedEvent::Kill { killer, .. } if *killer == 0),
                )
            });
            let terminal = cut_tick.unwrap_or(run_end);
            let (travelled, _, _) = trajectory_window(trace, 0, trail_tick.unwrap_or(1), terminal);
            checks.push(acceptance_check(
                "exposed-trail-evidence",
                trail_tick.is_some(),
                "human 1 must have a seeded authoritative trail or start one",
                format!(
                    "first exposed trail tick={trail_tick:?}; seeded={}",
                    initial_trail_tick.is_some()
                ),
            ));
            checks.push(acceptance_check(
                "swept-trail-cut",
                cut_tick.is_some(),
                "authoritative Death must identify NPC 0 cutting human 1's first trail",
                format!("first TrailCut death tick={cut_tick:?}"),
            ));
            checks.push(acceptance_check(
                "credited-kill",
                kill_tick.is_some(),
                "authoritative Kill must credit NPC 0 for the first cut",
                format!("first-cut same-tick kill={kill_tick:?}"),
            ));
            checks.push(acceptance_check(
                "approach-trajectory",
                travelled > 1.0,
                "NPC trajectory must contain actual movement toward the encounter",
                format!("NPC path length={travelled:.3}"),
            ));
        }
        EncounterFixture::BaitDisengagement => {
            let maneuver_start = first_outbound_tick(trace).unwrap_or(1);
            let return_tick = npc_tactic_seen(trace, "return", Some("threat"), maneuver_start);
            let terminal = first_return_terminal(expected, trace, return_tick, run_end);
            let (_, exposed_ticks, reentered) =
                trajectory_window(trace, 0, maneuver_start, terminal);
            let npc_deaths =
                npc_death_ticks_in(expected, 0, return_tick.unwrap_or(maneuver_start), terminal);
            let disengage = return_tick.is_some();
            let abandoned = first_abandonment_tick(trace, maneuver_start, terminal).is_some();
            checks.push(acceptance_check(
                "exposed-excursion",
                exposed_ticks >= 5,
                "NPC must spend at least five sampled ticks outside its territory",
                format!("outside-territory samples={exposed_ticks}"),
            ));
            checks.push(acceptance_check(
                "threat-disengagement",
                disengage,
                "NPC must apply a threat return after the bait approach",
                format!("first threat-return tactic tick={return_tick:?}"),
            ));
            checks.push(acceptance_check(
                "tactic-abandonment",
                abandoned,
                "the exposed outbound tactic must be abandoned by the production controller",
                format!("outbound-to-safety-return transition={abandoned}"),
            ));
            checks.push(acceptance_check(
                "owned-reentry",
                reentered,
                "NPC trajectory must re-enter owned ground after disengaging",
                format!("trajectory ownership re-entry={reentered}"),
            ));
            checks.push(acceptance_check(
                "no-self-cut",
                npc_deaths.is_empty(),
                "NPC must not die while disengaging",
                format!("NPC death events in first disengagement window={npc_deaths:?}"),
            ));
        }
        EncounterFixture::DistractedTerritory => {
            let leader_trail = first_event_tick(
                expected,
                |event| matches!(event, RecordedEvent::TrailStarted { player } if *player == 1),
            );
            let theft_tick =
                leader_trail.and_then(|start| first_positive_theft(expected, 0, start));
            let (travelled, _, _) = trajectory_window(
                trace,
                0,
                leader_trail.unwrap_or(1),
                theft_tick.unwrap_or(run_end),
            );
            checks.push(acceptance_check(
                "leader-exposed",
                leader_trail.is_some(),
                "leader human 1 must expose an authoritative trail",
                format!("first leader TrailStarted tick={leader_trail:?}"),
            ));
            checks.push(acceptance_check(
                "positive-theft",
                theft_tick.is_some(),
                "NPC capture event in the first leader encounter must report stolen_area > 0",
                format!("first positive NPC theft capture tick={theft_tick:?}"),
            ));
            checks.push(acceptance_check(
                "theft-window-motion",
                travelled > 1.0,
                "NPC must have an actual trajectory during the theft window",
                format!("NPC path length={travelled:.3}"),
            ));
        }
        EncounterFixture::LoopCapture => {
            let maneuver_start = first_outbound_tick(trace).unwrap_or(1);
            let capture_tick = first_positive_capture(expected, 0, true, maneuver_start);
            let (_, _, reentered) =
                trajectory_window(trace, 0, maneuver_start, capture_tick.unwrap_or(run_end));
            let preclosure_deaths = capture_tick.map_or_else(Vec::new, |closure| {
                npc_death_ticks_in(expected, 0, 1, closure.saturating_sub(1))
            });
            let changed = capture_tick.is_some_and(|tick| territory_changed_at(expected, tick));
            checks.push(acceptance_check(
                "authoritative-loop-fill",
                capture_tick.is_some(),
                "NPC must receive a positive-area Capture event with loop_fill=true",
                format!("first positive loop capture tick={capture_tick:?}"),
            ));
            checks.push(acceptance_check(
                "vector-territory-change",
                changed,
                "capture must change the authoritative vector territory fingerprint",
                format!("territory fingerprint changed={changed}"),
            ));
            checks.push(acceptance_check(
                "closure-reentry",
                reentered,
                "actual NPC trajectory must leave and re-enter owned ground before closure",
                format!("first-maneuver trajectory ownership re-entry={reentered}"),
            ));
            checks.push(acceptance_check(
                "no-preclosure-death",
                preclosure_deaths.is_empty(),
                "NPC must not die before the first positive loop closure",
                format!("NPC death events before first closure={preclosure_deaths:?}"),
            ));
        }
        EncounterFixture::BridgeCapture => {
            // This geometry is retained as a topology diagnostic only. It was
            // never authored as an NPC bridge route, so loop_fill=true is not
            // evidence that the NPC is wrong and must not fail acceptance.
            checks.push(acceptance_check(
                "bridge-topology-diagnostic",
                true,
                "bridge fixture is diagnostic only; no NPC capture semantics are asserted",
                "excluded from NPC acceptance; loop_fill is intentionally not classified",
            ));
        }
    }
    // Keep a scalar diagnostic tied to the same report so zero-event fixtures
    // cannot look healthy merely because the controller produced labels.
    checks.push(acceptance_check(
        "finite-motion",
        trace
            .trajectories
            .values()
            .flatten()
            .all(|point| point.iter().all(|value| value.is_finite())),
        "all recorded trajectory coordinates must be finite",
        format!(
            "tracked NPCs={} fixed ticks={}",
            trace.trajectories.len(),
            stats.fixed_ticks
        ),
    ));
    LabAcceptance {
        passed: checks.iter().all(|check| check.passed),
        checks,
    }
}

#[cfg(test)]
mod tests;

use super::super::model::{LAB_FORMAT_VERSION, LAB_SETUP_VERSION};
use std::collections::BTreeMap;

use crate::match_game::{ComparableCompetitor, ComparableSnapshot};
use bevy::prelude::default;

use super::{
    evaluate_acceptance, event_tick_at, first_event_tick, first_event_tick_after,
    first_positive_capture, npc_death_ticks_in, trajectory_window,
};
use crate::npc::lab::{
    EncounterFixture, LabAcceptance, LabDecisionSample, LabManeuverStats, LabReport, LabRunner,
    LabTrace, LabVariant, MAX_TICKS, PersonalityVariant, RecordedEvent, ReplayTick, write_svg,
};

#[test]
fn authored_encounters_produce_authoritative_first_maneuver_outcomes() {
    for (fixture, personality) in [
        (EncounterFixture::ReturnRace, PersonalityVariant::Builder),
        (EncounterFixture::Interception, PersonalityVariant::Hunter),
        (
            EncounterFixture::BaitDisengagement,
            PersonalityVariant::Builder,
        ),
        (
            EncounterFixture::DistractedTerritory,
            PersonalityVariant::Raider,
        ),
        (EncounterFixture::LoopCapture, PersonalityVariant::Builder),
    ] {
        let (field_seed, npc_seed) = fixture.default_seeds();
        let artifact = LabRunner::new(
            fixture,
            field_seed,
            npc_seed,
            LabVariant {
                personality,
                ..default()
            },
            600,
        )
        .unwrap()
        .run()
        .unwrap();
        assert!(
            artifact.report.acceptance.passed,
            "{}: {:?}",
            fixture.label(),
            artifact.report.acceptance
        );
    }
}

#[test]
fn fixture_names_and_seeds_are_stable() {
    assert_eq!(
        "return-race".parse::<EncounterFixture>().unwrap(),
        EncounterFixture::ReturnRace
    );
    assert_ne!(
        EncounterFixture::ReturnRace.default_seeds(),
        EncounterFixture::BridgeCapture.default_seeds()
    );
}

#[test]
fn ticks_are_bounded_without_running_the_simulation() {
    assert!(
        LabRunner::new(
            EncounterFixture::ReturnRace,
            1,
            2,
            LabVariant::default(),
            MAX_TICKS + 1
        )
        .is_err()
    );
    assert!(LabRunner::new(EncounterFixture::ReturnRace, 1, 2, LabVariant::default(), 0).is_err());
}

#[test]
fn decision_trace_is_a_ring_that_keeps_later_transitions() {
    let variant = LabVariant {
        trace_capacity: 1,
        ..LabVariant::default()
    };
    let artifact = LabRunner::new(EncounterFixture::ReturnRace, 1, 2, variant, 2)
        .unwrap()
        .run()
        .unwrap();
    assert_eq!(artifact.report.trace.decisions.len(), 1);
    assert_eq!(artifact.report.trace.decisions[0].tick, 2);
}

#[test]
fn acceptance_requires_authoritative_events_not_tactic_labels() {
    let trace = LabTrace {
        decisions: vec![LabDecisionSample {
            tick: 1,
            npc: 0,
            action: "Hunt(Segment)".into(),
            reason: "hunt".into(),
            tactic_kind: None,
            tactic_detail: None,
            tactic_phase: None,
            tactic_sequence: 0,
            tactic_started_tick: None,
            tactic_transition: false,
            previous_tactic_kind: None,
            position: [0.0, 0.0],
            heading: [1.0, 0.0],
            steering_error: 0.0,
            safety_override: false,
        }],
        trajectories: BTreeMap::from([(0, vec![[0.0, 0.0], [2.0, 0.0]])]),
        trajectory_ticks: BTreeMap::from([(0, vec![1, 2])]),
        trajectory_ownership: BTreeMap::from([(0, vec![true, false])]),
    };
    let acceptance = evaluate_acceptance(
        EncounterFixture::Interception,
        &[ReplayTick {
            snapshot: ComparableSnapshot::default(),
            events: Vec::new(),
        }],
        &trace,
        &LabManeuverStats::default(),
    );
    assert!(!acceptance.passed);
    assert!(
        acceptance
            .checks
            .iter()
            .any(|check| check.name == "swept-trail-cut" && !check.passed)
    );
}

#[test]
fn interception_accepts_seeded_trail_but_not_a_later_life_cut() {
    let mut expected = vec![
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 1,
                competitors: vec![ComparableCompetitor {
                    id: 1,
                    trail_length: 0.5,
                    ..default()
                }],
                ..default()
            },
            events: vec![],
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 2,
                ..default()
            },
            events: vec![
                RecordedEvent::Death {
                    victim: 1,
                    killer: Some(0),
                    cause: "TrailCut".into(),
                },
                RecordedEvent::Kill {
                    killer: 0,
                    total: 1,
                    streak: 1,
                },
            ],
        },
    ];
    let trace = LabTrace {
        trajectories: BTreeMap::from([(0, vec![[0.0, 0.0], [2.0, 0.0]])]),
        trajectory_ticks: BTreeMap::from([(0, vec![1, 2])]),
        trajectory_ownership: BTreeMap::from([(0, vec![true, false])]),
        ..default()
    };
    assert!(
        evaluate_acceptance(
            EncounterFixture::Interception,
            &expected,
            &trace,
            &default()
        )
        .passed
    );
    expected[0].events.push(RecordedEvent::Capture {
        player: 1,
        area: 1.0,
        stolen_area: 0.0,
        loop_fill: true,
    });
    assert!(
        !evaluate_acceptance(
            EncounterFixture::Interception,
            &expected,
            &trace,
            &default()
        )
        .passed
    );
}

#[test]
fn bridge_is_diagnostic_and_does_not_assert_loop_fill() {
    let acceptance = evaluate_acceptance(
        EncounterFixture::BridgeCapture,
        &[],
        &LabTrace::default(),
        &LabManeuverStats::default(),
    );
    assert!(acceptance.passed);
    assert!(
        acceptance
            .checks
            .iter()
            .any(|check| check.name == "bridge-topology-diagnostic" && check.passed)
    );
    assert!(
        !acceptance
            .checks
            .iter()
            .any(|check| check.name == "authoritative-bridge-capture")
    );
}

#[test]
fn loop_death_window_excludes_death_after_first_closure() {
    let expected = vec![
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 2,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Capture {
                player: 0,
                area: 2.0,
                stolen_area: 0.0,
                loop_fill: true,
            }],
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 8,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Death {
                victim: 0,
                killer: None,
                cause: "SelfTrail".into(),
            }],
        },
    ];
    let closure = first_positive_capture(&expected, 0, true, 1).unwrap();
    assert!(npc_death_ticks_in(&expected, 0, 1, closure - 1).is_empty());
    assert_eq!(npc_death_ticks_in(&expected, 0, 1, 8).len(), 1);
}

#[test]
fn loop_acceptance_ignores_death_after_first_closure() {
    let expected = vec![
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 1,
                territory_fingerprint: 10,
                ..ComparableSnapshot::default()
            },
            events: Vec::new(),
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 2,
                territory_fingerprint: 11,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Capture {
                player: 0,
                area: 2.0,
                stolen_area: 0.0,
                loop_fill: true,
            }],
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 8,
                territory_fingerprint: 11,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Death {
                victim: 0,
                killer: None,
                cause: "SelfTrail".into(),
            }],
        },
    ];
    let trace = LabTrace {
        decisions: vec![LabDecisionSample {
            tick: 1,
            npc: 0,
            action: "capture(fill-frontier)".into(),
            reason: "capture".into(),
            tactic_kind: Some("capture".into()),
            tactic_detail: Some("fill-frontier".into()),
            tactic_phase: Some("travelling".into()),
            tactic_sequence: 1,
            tactic_started_tick: Some(1),
            tactic_transition: true,
            previous_tactic_kind: None,
            position: [0.0, 0.0],
            heading: [1.0, 0.0],
            steering_error: 0.0,
            safety_override: false,
        }],
        trajectories: BTreeMap::from([(0, vec![[0.0, 0.0], [1.0, 0.0]])]),
        trajectory_ticks: BTreeMap::from([(0, vec![1, 2])]),
        trajectory_ownership: BTreeMap::from([(0, vec![false, true])]),
    };
    let acceptance = evaluate_acceptance(
        EncounterFixture::LoopCapture,
        &expected,
        &trace,
        &LabManeuverStats::default(),
    );
    assert!(
        acceptance
            .checks
            .iter()
            .any(|check| check.name == "no-preclosure-death" && check.passed)
    );
}

#[test]
fn interception_does_not_credit_a_later_cycle_to_the_first_cut() {
    let expected = vec![
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 1,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::TrailStarted { player: 1 }],
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 2,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Death {
                victim: 1,
                killer: Some(0),
                cause: "TrailCut".into(),
            }],
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 7,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Kill {
                killer: 0,
                total: 1,
                streak: 1,
            }],
        },
    ];
    let trail = first_event_tick(&expected, |event| {
        matches!(event, RecordedEvent::TrailStarted { player: 1 })
    });
    let cut = first_event_tick_after(
        &expected,
        trail.unwrap(),
        |event| matches!(event, RecordedEvent::Death { victim: 1, killer: Some(0), cause } if cause == "TrailCut"),
    );
    assert_eq!(cut, Some(2));
    assert_eq!(
        event_tick_at(&expected, cut.unwrap(), |event| {
            matches!(event, RecordedEvent::Kill { killer: 0, .. })
        }),
        None
    );
}

#[test]
fn first_positive_capture_does_not_attribute_a_later_cycle() {
    let expected = vec![
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 3,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Capture {
                player: 0,
                area: 1.0,
                stolen_area: 0.0,
                loop_fill: true,
            }],
        },
        ReplayTick {
            snapshot: ComparableSnapshot {
                tick: 9,
                ..ComparableSnapshot::default()
            },
            events: vec![RecordedEvent::Capture {
                player: 0,
                area: 4.0,
                stolen_area: 0.0,
                loop_fill: true,
            }],
        },
    ];
    assert_eq!(first_positive_capture(&expected, 0, true, 1), Some(3));
    assert_eq!(first_positive_capture(&expected, 0, true, 4), Some(9));
}

#[test]
fn travel_window_stops_at_first_terminal_tick() {
    let trace = LabTrace {
        trajectories: BTreeMap::from([(0, vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]])]),
        trajectory_ticks: BTreeMap::from([(0, vec![1, 2, 3, 4])]),
        trajectory_ownership: BTreeMap::from([(0, vec![true, false, false, true])]),
        ..LabTrace::default()
    };
    let (travelled, exposed, reentered) = trajectory_window(&trace, 0, 1, 3);
    assert_eq!(travelled, 2.0);
    assert_eq!(exposed, 2);
    assert!(!reentered);
}

#[test]
fn svg_writer_is_bounded_to_recorded_trajectory() {
    let report = LabReport {
        format_version: LAB_FORMAT_VERSION,
        setup_version: LAB_SETUP_VERSION,
        fixture: EncounterFixture::ReturnRace,
        field_seed: 1,
        npc_roster_seed: 2,
        variant: LabVariant::default(),
        board_generation: 3,
        setup_hash: 4,
        final_snapshot: ComparableSnapshot::default(),
        stats: LabManeuverStats::default(),
        trace: LabTrace {
            decisions: Vec::new(),
            trajectories: BTreeMap::new(),
            trajectory_ticks: BTreeMap::new(),
            trajectory_ownership: BTreeMap::new(),
        },
        acceptance: LabAcceptance::default(),
    };
    let path = std::env::temp_dir().join("turfrace-npc-lab-test.svg");
    write_svg(&path, &report).unwrap();
    assert!(std::fs::metadata(path).is_ok());
}

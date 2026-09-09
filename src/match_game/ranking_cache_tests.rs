use super::*;

#[test]
fn idle_rankings_are_not_rewritten_but_relevant_inputs_invalidate_them() {
    let mut app = App::new();
    let board = BoardGrid::generate(7, 2, &GameConfig::default());
    app.insert_resource(TerritoryMap::from_board(&board))
        .insert_resource(board)
        .init_resource::<Rankings>()
        .init_resource::<SimulationEvents>()
        .add_systems(Update, update_rankings);
    let racer = app
        .world_mut()
        .spawn((
            Competitor {
                id: CompetitorId(0),
                display_name: "Racer".into(),
                kind: CompetitorKind::Human,
                color_id: 0,
                pattern_id: 0,
            },
            LifeState::alive(),
            MatchStatistics::default(),
        ))
        .id();
    app.update();
    let first_change = app
        .world()
        .get_resource_ref::<Rankings>()
        .unwrap()
        .last_changed();
    app.world_mut()
        .get_mut::<MatchStatistics>(racer)
        .unwrap()
        .time_alive_seconds = 10.0;
    app.update();
    assert_eq!(
        app.world()
            .get_resource_ref::<Rankings>()
            .unwrap()
            .last_changed(),
        first_change
    );

    app.world_mut()
        .get_mut::<MatchStatistics>(racer)
        .unwrap()
        .kills = 1;
    app.update();
    assert_eq!(app.world().resource::<Rankings>().entries[0].kills, 1);
    app.world_mut().get_mut::<LifeState>(racer).unwrap().status = LifeStatus::Respawning;
    app.update();
    assert!(!app.world().resource::<Rankings>().entries[0].alive);

    // Direct resource replacement must invalidate even with an identical
    // revision number (e.g. replaying the same field).
    let mut map = app.world().resource::<TerritoryMap>().clone();
    let arena = map.arena().clone();
    map.apply_claim(CompetitorId(0), arena);
    app.insert_resource(map);
    app.update();
    assert!(
        (app.world().resource::<Rankings>().entries[0].territory_percent - 100.0).abs() < 0.001
    );

    app.world_mut().despawn(racer);
    app.update();
    assert!(app.world().resource::<Rankings>().entries.is_empty());
}

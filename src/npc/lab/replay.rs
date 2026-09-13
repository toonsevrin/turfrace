use super::{
    model::{LAB_FORMAT_VERSION, LAB_SETUP_VERSION, LabError, LabReplay, MAX_TICKS},
    runner::LabRunner,
};

/// Reconstructs the fixture and compares every recorded fixed tick.  This is
/// intentionally strict about setup identity and reports the first mismatch.
pub fn verify_replay(replay: &LabReplay) -> Result<(), LabError> {
    if replay.format_version != LAB_FORMAT_VERSION || replay.setup_version != LAB_SETUP_VERSION {
        return Err(LabError::InvalidSetup(format!(
            "unsupported replay format/setup {}/{}",
            replay.format_version, replay.setup_version
        )));
    }
    if replay.ticks == 0
        || replay.ticks > MAX_TICKS
        || replay.expected.len() != replay.ticks as usize
    {
        return Err(LabError::InvalidSetup(
            "replay tick count is invalid or incomplete".into(),
        ));
    }
    let mut runner = LabRunner::new(
        replay.fixture,
        replay.field_seed,
        replay.npc_roster_seed,
        replay.variant.clone(),
        replay.ticks,
    )?;
    if runner.spec() != &replay.spec {
        return Err(LabError::InvalidSetup(
            "recorded MatchSpec differs from fixture setup".into(),
        ));
    }
    if runner.setup_identity() != (replay.board_generation, replay.setup_hash) {
        return Err(LabError::InvalidSetup(
            "recorded board/setup identity differs from fixture setup".into(),
        ));
    }
    for (index, expected) in replay.expected.iter().enumerate() {
        let tick = index as u64 + 1;
        let commands = replay
            .human_commands
            .iter()
            .copied()
            .filter(|command| command.tick == tick)
            .collect::<Vec<_>>();
        let actual = runner.step(commands, tick)?;
        if actual != *expected {
            return Err(LabError::InvalidSetup(format!(
                "replay diverged at tick {tick}"
            )));
        }
    }
    Ok(())
}

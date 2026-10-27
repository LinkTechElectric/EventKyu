use crate::{
    AutoContinue, Cue, CueError, CueKind, CueState, Engine, EngineEvent, FadeCurve, ManualClock,
    MediaAction,
};

fn fixture() -> (Engine, ManualClock) {
    let clock = ManualClock::new(0);
    let engine = Engine::with_clock(clock.clone());
    (engine, clock)
}

fn wait(id: u64, duration_ms: u64) -> Cue {
    Cue::new(id, format!("cue-{id}"), CueKind::Wait).with_duration_ms(duration_ms)
}

fn audio(id: u64) -> Cue {
    Cue::new(
        id,
        format!("media-{id}"),
        CueKind::audio(format!("{id}.wav")),
    )
}

fn advance(engine: &mut Engine, clock: &ManualClock, delta_ms: u64) {
    clock.advance(delta_ms);
    engine.tick_now().unwrap();
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

#[test]
fn pre_wait_action_and_post_wait_have_exact_timing() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(wait(1, 100).with_pre_wait_ms(50).with_post_wait_ms(25))
        .unwrap();
    engine.start(1).unwrap();
    assert_eq!(engine.state(1), Some(CueState::PreWait));
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(50));

    advance(&mut engine, &clock, 49);
    assert_eq!(engine.state(1), Some(CueState::PreWait));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::Running));
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(100));

    advance(&mut engine, &clock, 100);
    assert_eq!(engine.state(1), Some(CueState::PostWait));
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(25));
    advance(&mut engine, &clock, 24);
    assert_eq!(engine.state(1), Some(CueState::PostWait));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::Completed));

    assert!(engine.drain_events().iter().any(|event| matches!(
        event,
        EngineEvent::CueCompleted {
            cue_id: 1,
            at_ms: 175
        }
    )));
}

#[test]
fn auto_continue_counts_post_wait_from_action_start() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(
            wait(1, 1_000)
                .with_pre_wait_ms(100)
                .with_post_wait_ms(200)
                .with_auto_continue(AutoContinue::Continue),
        )
        .unwrap();
    engine.add_cue(wait(2, 50)).unwrap();
    engine.start(1).unwrap();

    advance(&mut engine, &clock, 299);
    assert_eq!(engine.state(2), Some(CueState::Stopped));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::Running));
    assert_eq!(engine.state(2), Some(CueState::Running));
}

#[test]
fn auto_follow_starts_next_when_action_completes() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(
            wait(1, 100)
                .with_post_wait_ms(50)
                .with_auto_continue(AutoContinue::Follow),
        )
        .unwrap();
    engine.add_cue(wait(2, 25)).unwrap();
    engine.start(1).unwrap();

    advance(&mut engine, &clock, 99);
    assert_eq!(engine.state(2), Some(CueState::Stopped));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::PostWait));
    assert_eq!(engine.state(2), Some(CueState::Running));
    advance(&mut engine, &clock, 25);
    assert_eq!(engine.state(2), Some(CueState::Completed));
    assert_eq!(engine.state(1), Some(CueState::PostWait));
    advance(&mut engine, &clock, 25);
    assert_eq!(engine.state(1), Some(CueState::Completed));
}

#[test]
fn large_tick_settles_a_multi_cue_follow_chain_at_real_deadlines() {
    let (mut engine, clock) = fixture();
    for (id, duration) in [(1, 10), (2, 20), (3, 30)] {
        engine
            .add_cue(wait(id, duration).with_auto_continue(AutoContinue::Follow))
            .unwrap();
    }
    engine.start(1).unwrap();
    advance(&mut engine, &clock, 100);

    assert_eq!(engine.state(1), Some(CueState::Completed));
    assert_eq!(engine.state(2), Some(CueState::Completed));
    assert_eq!(engine.state(3), Some(CueState::Completed));
    let completions: Vec<_> = engine
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            EngineEvent::CueCompleted { cue_id, at_ms } => Some((cue_id, at_ms)),
            _ => None,
        })
        .collect();
    assert_eq!(completions, vec![(1, 10), (2, 30), (3, 60)]);
}

#[test]
fn pause_resume_restores_pre_wait_remaining_time() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 50).with_pre_wait_ms(100)).unwrap();
    engine.start(1).unwrap();
    advance(&mut engine, &clock, 25);
    engine.pause(1).unwrap();
    assert_eq!(engine.state(1), Some(CueState::Paused));
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(75));

    advance(&mut engine, &clock, 100);
    assert_eq!(engine.state(1), Some(CueState::Paused));
    engine.resume(1).unwrap();
    advance(&mut engine, &clock, 74);
    assert_eq!(engine.state(1), Some(CueState::PreWait));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::Running));
}

#[test]
fn pause_resume_restores_action_remaining_time() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 100)).unwrap();
    engine.start(1).unwrap();
    advance(&mut engine, &clock, 40);
    engine.pause(1).unwrap();
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(60));

    advance(&mut engine, &clock, 500);
    engine.resume(1).unwrap();
    advance(&mut engine, &clock, 59);
    assert_eq!(engine.state(1), Some(CueState::Running));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::Completed));
}

#[test]
fn pause_resume_freezes_auto_continue_timer() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(
            wait(1, 1_000)
                .with_post_wait_ms(100)
                .with_auto_continue(AutoContinue::Continue),
        )
        .unwrap();
    engine.add_cue(wait(2, 10)).unwrap();
    engine.start(1).unwrap();
    advance(&mut engine, &clock, 40);
    engine.pause(1).unwrap();
    advance(&mut engine, &clock, 60);
    assert_eq!(engine.state(2), Some(CueState::Stopped));

    engine.resume(1).unwrap();
    advance(&mut engine, &clock, 59);
    assert_eq!(engine.state(2), Some(CueState::Stopped));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(2), Some(CueState::Running));
}

#[test]
fn faded_start_interpolates_all_curve_midpoints() {
    for (curve, expected) in [
        (FadeCurve::Linear, 0.5),
        (FadeCurve::EqualPower, 0.5_f32.sqrt()),
        (FadeCurve::SCurve, 0.5),
    ] {
        let (mut engine, clock) = fixture();
        engine.add_cue(wait(1, 1_000)).unwrap();
        engine.start_faded(1, 100, curve).unwrap();
        advance(&mut engine, &clock, 50);
        let status = engine.status(1).unwrap();
        close(status.volume, expected);
        close(status.opacity, expected);
        advance(&mut engine, &clock, 50);
        close(engine.status(1).unwrap().volume, 1.0);
    }
}

#[test]
fn faded_start_respects_configured_parameter_endpoints() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(wait(1, 1_000).with_volume(0.8).with_opacity(0.6))
        .unwrap();
    engine.start_faded(1, 100, FadeCurve::Linear).unwrap();
    advance(&mut engine, &clock, 50);
    let status = engine.status(1).unwrap();
    close(status.volume, 0.4);
    close(status.opacity, 0.3);
}

#[test]
fn faded_start_begins_after_pre_wait() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 1_000).with_pre_wait_ms(50)).unwrap();
    engine.start_faded(1, 100, FadeCurve::Linear).unwrap();
    advance(&mut engine, &clock, 50);
    assert_eq!(engine.state(1), Some(CueState::Running));
    close(engine.status(1).unwrap().volume, 0.0);
    advance(&mut engine, &clock, 50);
    close(engine.status(1).unwrap().volume, 0.5);
}

#[test]
fn faded_stop_interpolates_all_curve_midpoints_then_stops() {
    for (curve, expected) in [
        (FadeCurve::Linear, 0.5),
        (FadeCurve::EqualPower, 0.5_f32.sqrt()),
        (FadeCurve::SCurve, 0.5),
    ] {
        let (mut engine, clock) = fixture();
        engine.add_cue(wait(1, 1_000)).unwrap();
        engine.start(1).unwrap();
        engine.stop_faded(1, 100, curve).unwrap();
        advance(&mut engine, &clock, 50);
        assert_eq!(engine.state(1), Some(CueState::Running));
        close(engine.status(1).unwrap().volume, expected);
        advance(&mut engine, &clock, 50);
        assert_eq!(engine.state(1), Some(CueState::Stopped));
        close(engine.status(1).unwrap().volume, 0.0);
    }
}

#[test]
fn faded_pause_and_resume_interpolate_and_preserve_action_time() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 1_000)).unwrap();
    engine.start(1).unwrap();
    engine.pause_faded(1, 100, FadeCurve::EqualPower).unwrap();
    advance(&mut engine, &clock, 50);
    assert_eq!(engine.state(1), Some(CueState::Running));
    close(engine.status(1).unwrap().volume, 0.5_f32.sqrt());
    advance(&mut engine, &clock, 50);
    assert_eq!(engine.state(1), Some(CueState::Paused));
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(900));
    close(engine.status(1).unwrap().volume, 0.0);

    engine.resume_faded(1, 100, FadeCurve::Linear).unwrap();
    advance(&mut engine, &clock, 50);
    assert_eq!(engine.state(1), Some(CueState::Running));
    close(engine.status(1).unwrap().volume, 0.5);
    advance(&mut engine, &clock, 50);
    close(engine.status(1).unwrap().volume, 1.0);
    assert_eq!(engine.status(1).unwrap().phase_remaining_ms, Some(800));
}

#[test]
fn pausing_a_fade_freezes_interpolation() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 1_000)).unwrap();
    engine.start_faded(1, 100, FadeCurve::Linear).unwrap();
    advance(&mut engine, &clock, 40);
    engine.pause(1).unwrap();
    close(engine.status(1).unwrap().volume, 0.4);
    advance(&mut engine, &clock, 200);
    close(engine.status(1).unwrap().volume, 0.4);

    engine.resume(1).unwrap();
    advance(&mut engine, &clock, 59);
    close(engine.status(1).unwrap().volume, 0.99);
    advance(&mut engine, &clock, 1);
    close(engine.status(1).unwrap().volume, 1.0);
}

#[test]
fn fade_cue_changes_target_volume_and_opacity() {
    let (mut engine, clock) = fixture();
    engine.add_cue(audio(1)).unwrap();
    engine
        .add_cue(
            Cue::new(
                2,
                "fade target",
                CueKind::Fade {
                    target: 1,
                    volume: Some(0.0),
                    opacity: Some(0.25),
                    curve: FadeCurve::Linear,
                },
            )
            .with_duration_ms(100),
        )
        .unwrap();
    engine.start(1).unwrap();
    engine.start(2).unwrap();

    advance(&mut engine, &clock, 50);
    let target = engine.status(1).unwrap();
    close(target.volume, 0.5);
    close(target.opacity, 0.625);
    advance(&mut engine, &clock, 50);
    let target = engine.status(1).unwrap();
    close(target.volume, 0.0);
    close(target.opacity, 0.25);
    assert_eq!(engine.state(2), Some(CueState::Completed));
}

#[test]
fn go_triggers_and_advances_only_the_user_playhead() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(wait(1, 100).with_auto_continue(AutoContinue::Follow))
        .unwrap();
    engine.add_cue(wait(2, 100)).unwrap();
    engine.add_cue(wait(3, 100)).unwrap();

    assert_eq!(engine.go().unwrap(), 1);
    assert_eq!(engine.playhead_cue(), Some(2));
    advance(&mut engine, &clock, 100);
    assert_eq!(engine.state(2), Some(CueState::Running));
    assert_eq!(engine.playhead_cue(), Some(2));
    assert_eq!(engine.state(3), Some(CueState::Stopped));
}

#[test]
fn successive_go_operations_follow_list_order() {
    let (mut engine, _clock) = fixture();
    engine.add_cue(audio(1)).unwrap();
    engine.add_cue(audio(2)).unwrap();
    assert_eq!(engine.go().unwrap(), 1);
    assert_eq!(engine.go().unwrap(), 2);
    assert_eq!(engine.playhead_cue(), None);
    assert_eq!(engine.go(), Err(CueError::NoCueAtPlayhead));
}

#[test]
fn cart_cues_run_concurrently_and_toggle_independently() {
    let (mut engine, clock) = fixture();
    engine.add_cue(audio(1)).unwrap();
    engine.add_cue(audio(2)).unwrap();
    engine.bind_cart(10, 1).unwrap();
    engine.bind_cart(20, 2).unwrap();

    assert_eq!(engine.trigger_cart(10).unwrap(), 1);
    assert_eq!(engine.trigger_cart(20).unwrap(), 2);
    assert_eq!(engine.state(1), Some(CueState::Running));
    assert_eq!(engine.state(2), Some(CueState::Running));
    assert_eq!(engine.playhead_cue(), Some(1));
    advance(&mut engine, &clock, 50);
    assert_eq!(engine.state(1), Some(CueState::Running));
    assert_eq!(engine.state(2), Some(CueState::Running));

    engine.toggle_cart(10).unwrap();
    assert_eq!(engine.state(1), Some(CueState::Stopped));
    assert_eq!(engine.state(2), Some(CueState::Running));
    engine.toggle_cart(10).unwrap();
    assert_eq!(engine.state(1), Some(CueState::Running));
}

#[test]
fn media_without_duration_waits_for_external_completion() {
    let (mut engine, clock) = fixture();
    engine
        .add_cue(audio(1).with_auto_continue(AutoContinue::Follow))
        .unwrap();
    engine.add_cue(audio(2)).unwrap();
    engine.start(1).unwrap();
    advance(&mut engine, &clock, 10_000);
    assert_eq!(engine.state(1), Some(CueState::Running));
    assert_eq!(engine.state(2), Some(CueState::Stopped));

    engine.notify_media_completed(1).unwrap();
    assert_eq!(engine.state(1), Some(CueState::Completed));
    assert_eq!(engine.state(2), Some(CueState::Running));
    engine.notify_media_completed(2).unwrap();
    assert_eq!(engine.state(2), Some(CueState::Completed));
}

#[test]
fn media_with_duration_can_complete_without_external_event() {
    let (mut engine, clock) = fixture();
    engine.add_cue(audio(1).with_duration_ms(80)).unwrap();
    engine.start(1).unwrap();
    advance(&mut engine, &clock, 79);
    assert_eq!(engine.state(1), Some(CueState::Running));
    advance(&mut engine, &clock, 1);
    assert_eq!(engine.state(1), Some(CueState::Completed));
}

#[test]
fn media_adapter_receives_play_pause_resume_and_stop_requests() {
    let (mut engine, _clock) = fixture();
    engine.add_cue(audio(1)).unwrap();
    engine.start(1).unwrap();
    engine.pause(1).unwrap();
    engine.resume(1).unwrap();
    engine.stop(1).unwrap();
    let actions: Vec<_> = engine
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            EngineEvent::MediaActionRequested { action, .. } => Some(action),
            _ => None,
        })
        .collect();
    assert_eq!(
        actions,
        vec![
            MediaAction::Play,
            MediaAction::Pause,
            MediaAction::Resume,
            MediaAction::Stop
        ]
    );
}

#[test]
fn group_runs_children_sequentially_including_post_wait() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 10).with_post_wait_ms(5)).unwrap();
    engine.add_cue(wait(2, 20)).unwrap();
    engine
        .add_cue(Cue::new(
            3,
            "group",
            CueKind::Group {
                children: vec![1, 2],
            },
        ))
        .unwrap();
    engine.start(3).unwrap();
    assert_eq!(engine.state(3), Some(CueState::Running));
    assert_eq!(engine.state(1), Some(CueState::Running));

    advance(&mut engine, &clock, 10);
    assert_eq!(engine.state(1), Some(CueState::PostWait));
    assert_eq!(engine.state(2), Some(CueState::Stopped));
    advance(&mut engine, &clock, 5);
    assert_eq!(engine.state(2), Some(CueState::Running));
    advance(&mut engine, &clock, 20);
    assert_eq!(engine.state(3), Some(CueState::Completed));
}

#[test]
fn group_cycle_is_an_error() {
    let (mut engine, _clock) = fixture();
    engine
        .add_cue(Cue::new(1, "cycle", CueKind::Group { children: vec![1] }))
        .unwrap();
    assert_eq!(engine.start(1), Err(CueError::GroupCycle(1)));
}

#[test]
fn fade_target_errors_propagate_and_enter_error_state() {
    let (mut engine, _clock) = fixture();
    engine.add_cue(wait(1, 100)).unwrap();
    engine
        .add_cue(
            Cue::new(
                2,
                "bad fade",
                CueKind::Fade {
                    target: 1,
                    volume: Some(0.0),
                    opacity: None,
                    curve: FadeCurve::Linear,
                },
            )
            .with_duration_ms(100),
        )
        .unwrap();
    assert_eq!(engine.start(2), Err(CueError::TargetNotRunning(1)));
    assert_eq!(engine.state(2), Some(CueState::Error));
    assert!(engine.drain_events().iter().any(|event| matches!(
        event,
        EngineEvent::Error {
            error: CueError::TargetNotRunning(1),
            ..
        }
    )));
}

#[test]
fn unknown_cue_errors_are_returned_and_emitted() {
    let (mut engine, _clock) = fixture();
    assert_eq!(engine.start(99), Err(CueError::UnknownCue(99)));
    assert!(engine.drain_events().iter().any(|event| matches!(
        event,
        EngineEvent::Error {
            error: CueError::UnknownCue(99),
            ..
        }
    )));
}

#[test]
fn invalid_external_media_completion_is_an_error() {
    let (mut engine, _clock) = fixture();
    engine.add_cue(wait(1, 100)).unwrap();
    engine.start(1).unwrap();
    assert_eq!(
        engine.notify_media_completed(1),
        Err(CueError::InvalidMediaCompletion(1))
    );
}

#[test]
fn wait_requires_a_finite_duration() {
    let (mut engine, _clock) = fixture();
    engine
        .add_cue(Cue::new(1, "bad wait", CueKind::Wait))
        .unwrap();
    assert_eq!(engine.start(1), Err(CueError::DurationRequired(1)));
}

#[test]
fn stop_terminates_a_paused_cue() {
    let (mut engine, _clock) = fixture();
    engine.add_cue(audio(1)).unwrap();
    engine.start(1).unwrap();
    engine.pause(1).unwrap();
    engine.stop(1).unwrap();
    assert_eq!(engine.state(1), Some(CueState::Stopped));
}

#[test]
fn non_monotonic_manual_tick_is_rejected() {
    let (mut engine, clock) = fixture();
    advance(&mut engine, &clock, 10);
    assert_eq!(
        engine.tick(9),
        Err(CueError::NonMonotonicClock {
            previous_ms: 10,
            received_ms: 9,
        })
    );
    assert_eq!(engine.now_ms(), 10);
}

#[test]
fn fade_updates_are_emitted_with_current_values() {
    let (mut engine, clock) = fixture();
    engine.add_cue(wait(1, 1_000)).unwrap();
    engine.start_faded(1, 100, FadeCurve::Linear).unwrap();
    engine.drain_events();
    advance(&mut engine, &clock, 50);
    assert!(engine.drain_events().iter().any(|event| matches!(
        event,
        EngineEvent::FadeUpdated { cue_id: 1, volume, .. } if (*volume - 0.5).abs() < 1e-6
    )));
}

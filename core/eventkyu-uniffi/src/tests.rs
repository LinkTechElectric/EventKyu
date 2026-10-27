use super::*;

#[test]
fn exports_version_and_adds_typed_cues() {
    assert_eq!(eventkyu_version(), env!("CARGO_PKG_VERSION"));
    let engine = CueEngineHandle::new();
    let audio_id = engine
        .add_cue(
            CueKind::Audio,
            "Opening".into(),
            "opening.wav".into(),
            0,
            None,
            0,
            None,
        )
        .unwrap();
    let video_id = engine
        .add_cue(
            CueKind::Video,
            "PV".into(),
            "pv.mp4".into(),
            0,
            Some(1_000),
            0,
            Some(AutoContinue::None),
        )
        .unwrap();
    let wait_id = engine
        .add_cue(
            CueKind::Wait,
            "MC".into(),
            String::new(),
            0,
            Some(0),
            0,
            None,
        )
        .unwrap();

    assert_eq!(
        (audio_id.as_str(), video_id.as_str(), wait_id.as_str()),
        ("1", "2", "3")
    );
    let status = engine.status().unwrap();
    assert_eq!(status.playhead_cue_id.as_deref(), Some("1"));
    assert_eq!(status.cues.len(), 3);
    assert_eq!(status.cues[0].kind, "Audio");
    assert_eq!(status.cues[1].kind, "Video");
    assert_eq!(status.cues[2].kind, "Wait");
}

#[test]
fn go_and_events_work_through_the_handle() {
    let engine = CueEngineHandle::new();
    let id = engine
        .add_cue(
            CueKind::Audio,
            "Theme".into(),
            "theme.wav".into(),
            0,
            None,
            0,
            None,
        )
        .unwrap();

    assert_eq!(engine.go().unwrap(), id);
    assert_eq!(engine.status().unwrap().cues[0].state, CueState::Running);
    assert_eq!(engine.status().unwrap().playhead_cue_id, None);

    let events = engine.drain_events().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].kind, EngineEventKind::CueStateChanged);
    assert_eq!(events[0].state, Some(CueState::PreWait));
    assert_eq!(events[1].state, Some(CueState::Running));
    assert_eq!(events[2].kind, EngineEventKind::MediaActionRequested);
    assert_eq!(events[2].media_action, Some(MediaAction::Play));
    assert!(engine.drain_events().unwrap().is_empty());
}

#[test]
fn faded_transport_methods_preserve_engine_semantics() {
    let engine = CueEngineHandle::new();
    let id = engine
        .add_cue(
            CueKind::Video,
            "PV".into(),
            "pv.mp4".into(),
            0,
            None,
            0,
            None,
        )
        .unwrap();

    engine.start(id.clone(), Some(0)).unwrap();
    assert_eq!(engine.status().unwrap().cues[0].state, CueState::Running);
    engine.pause(id.clone(), Some(0)).unwrap();
    assert_eq!(engine.status().unwrap().cues[0].state, CueState::Paused);
    engine.resume(id.clone(), Some(0)).unwrap();
    assert_eq!(engine.status().unwrap().cues[0].state, CueState::Running);
    engine.stop(id, Some(0)).unwrap();
    assert_eq!(engine.status().unwrap().cues[0].state, CueState::Stopped);

    let actions: Vec<_> = engine
        .drain_events()
        .unwrap()
        .into_iter()
        .filter_map(|event| event.media_action)
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
fn zero_duration_wait_completes_without_sleeping() {
    let engine = CueEngineHandle::new();
    engine
        .add_cue(
            CueKind::Wait,
            "Blackout".into(),
            String::new(),
            0,
            Some(0),
            0,
            None,
        )
        .unwrap();
    engine.go().unwrap();
    assert_eq!(engine.status().unwrap().cues[0].state, CueState::Completed);
    assert!(engine
        .drain_events()
        .unwrap()
        .iter()
        .any(|event| event.kind == EngineEventKind::CueCompleted));
}

#[test]
fn malformed_ids_are_swift_errors_not_panics() {
    let engine = CueEngineHandle::new();
    let error = engine.start("not-a-number".into(), None).unwrap_err();
    assert_eq!(error, EventKyuError::InvalidCueId("not-a-number".into()));
    assert!(engine.drain_events().unwrap().is_empty());
}

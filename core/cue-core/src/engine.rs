use crate::{
    AutoContinue, Clock, Cue, CueError, CueId, CueKind, CueState, EngineEvent, FadeCurve,
    MediaAction, SystemClock,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A read-only snapshot of a cue's runtime state.
#[derive(Debug, Clone, PartialEq)]
pub struct CueStatus {
    /// Cue identifier.
    pub cue_id: CueId,
    /// Current public state.
    pub state: CueState,
    /// Current interpolated volume.
    pub volume: f32,
    /// Current interpolated opacity.
    pub opacity: f32,
    /// Remaining time in the current phase, or `None` for an open-ended action.
    pub phase_remaining_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    PreWait,
    Running,
    PostWait,
}

impl Phase {
    const fn state(self) -> CueState {
        match self {
            Self::PreWait => CueState::PreWait,
            Self::Running => CueState::Running,
            Self::PostWait => CueState::PostWait,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadePurpose {
    Start,
    Stop,
    Pause,
    Parameters,
}

#[derive(Debug, Clone)]
struct ActiveFade {
    started_ms: u64,
    duration_ms: u64,
    curve: FadeCurve,
    from_volume: f32,
    to_volume: f32,
    from_opacity: f32,
    to_opacity: f32,
    purpose: FadePurpose,
}

impl ActiveFade {
    fn end_ms(&self) -> u64 {
        self.started_ms.saturating_add(self.duration_ms)
    }

    fn elapsed_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.started_ms).min(self.duration_ms)
    }

    fn values(&self, now_ms: u64) -> (f32, f32) {
        let progress = if self.duration_ms == 0 {
            1.0
        } else {
            self.elapsed_ms(now_ms) as f32 / self.duration_ms as f32
        };
        (
            interpolate(self.from_volume, self.to_volume, progress, self.curve),
            interpolate(self.from_opacity, self.to_opacity, progress, self.curve),
        )
    }
}

fn interpolate(from: f32, to: f32, progress: f32, curve: FadeCurve) -> f32 {
    if to >= from {
        from + (to - from) * curve.evaluate(progress)
    } else {
        // Evaluate the reversed ramp for decreasing values. In particular,
        // EqualPower then becomes a cosine-style fade-out rather than a fast dip.
        to + (from - to) * curve.evaluate(1.0 - progress)
    }
}

#[derive(Debug, Clone)]
struct GroupProgress {
    children: Vec<CueId>,
    next_index: usize,
    waiting_on: Option<CueId>,
}

#[derive(Debug, Clone)]
struct Runtime {
    state: CueState,
    phase: Option<Phase>,
    phase_end_ms: Option<u64>,
    paused_phase_remaining_ms: Option<u64>,
    continue_due_ms: Option<u64>,
    paused_continue_remaining_ms: Option<u64>,
    fade: Option<ActiveFade>,
    paused_fade_elapsed_ms: Option<u64>,
    pending_start_fade: Option<(u64, FadeCurve)>,
    group: Option<GroupProgress>,
    volume: f32,
    opacity: f32,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            state: CueState::Stopped,
            phase: None,
            phase_end_ms: None,
            paused_phase_remaining_ms: None,
            continue_due_ms: None,
            paused_continue_remaining_ms: None,
            fade: None,
            paused_fade_elapsed_ms: None,
            pending_start_fade: None,
            group: None,
            volume: 1.0,
            opacity: 1.0,
        }
    }
}

impl Runtime {
    fn is_active(&self) -> bool {
        matches!(
            self.state,
            CueState::PreWait | CueState::Running | CueState::PostWait | CueState::Paused
        )
    }

    fn phase_remaining_ms(&self, now_ms: u64) -> Option<u64> {
        if self.state == CueState::Paused {
            self.paused_phase_remaining_ms
        } else {
            self.phase_end_ms
                .map(|end_ms| end_ms.saturating_sub(now_ms))
        }
    }
}

/// Concurrent cue engine with list, cart, timing, fade, and event support.
///
/// Public triggering methods synchronize to the injected clock before acting.
/// [`tick`](Self::tick) can alternatively drive the engine with explicit
/// monotonic timestamps, which is useful for deterministic simulations.
pub struct Engine {
    cues: BTreeMap<CueId, Cue>,
    runtimes: BTreeMap<CueId, Runtime>,
    list: Vec<CueId>,
    playhead: Option<usize>,
    cart: BTreeMap<u32, CueId>,
    events: VecDeque<EngineEvent>,
    clock: Box<dyn Clock>,
    now_ms: u64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Creates an empty engine driven by [`SystemClock`].
    pub fn new() -> Self {
        Self::with_clock(SystemClock::new())
    }

    /// Creates an empty engine driven by the supplied clock.
    pub fn with_clock<C>(clock: C) -> Self
    where
        C: Clock + 'static,
    {
        let now_ms = clock.now_ms();
        Self {
            cues: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            list: Vec::new(),
            playhead: None,
            cart: BTreeMap::new(),
            events: VecDeque::new(),
            clock: Box::new(clock),
            now_ms,
        }
    }

    /// Adds a cue and appends it to the list.
    ///
    /// The first added cue becomes the playhead selection.
    pub fn add_cue(&mut self, cue: Cue) -> Result<(), CueError> {
        let id = cue.id();
        if self.cues.contains_key(&id) {
            return Err(self.fail_at(Some(id), CueError::DuplicateCue(id), self.now_ms));
        }
        self.runtimes.insert(
            id,
            Runtime {
                volume: cue.volume(),
                opacity: cue.opacity(),
                ..Runtime::default()
            },
        );
        self.list.push(id);
        if self.playhead.is_none() {
            self.playhead = Some(self.list.len() - 1);
        }
        self.cues.insert(id, cue);
        Ok(())
    }

    /// Returns a cue definition by identifier.
    pub fn cue(&self, id: CueId) -> Option<&Cue> {
        self.cues.get(&id)
    }

    /// Returns a runtime snapshot, or an error for an unknown cue.
    pub fn status(&self, id: CueId) -> Result<CueStatus, CueError> {
        let runtime = self.runtimes.get(&id).ok_or(CueError::UnknownCue(id))?;
        Ok(CueStatus {
            cue_id: id,
            state: runtime.state,
            volume: runtime.volume,
            opacity: runtime.opacity,
            phase_remaining_ms: runtime.phase_remaining_ms(self.now_ms),
        })
    }

    /// Returns a cue's state, or `None` for an unknown cue.
    pub fn state(&self, id: CueId) -> Option<CueState> {
        self.runtimes.get(&id).map(|runtime| runtime.state)
    }

    /// Returns the cue list in trigger order.
    pub fn list(&self) -> &[CueId] {
        &self.list
    }

    /// Returns the selected playhead index.
    pub const fn playhead_index(&self) -> Option<usize> {
        self.playhead
    }

    /// Returns the cue selected by the playhead.
    pub fn playhead_cue(&self) -> Option<CueId> {
        self.playhead
            .and_then(|index| self.list.get(index).copied())
    }

    /// Selects a list position without affecting any running cue.
    pub fn set_playhead(&mut self, index: usize) -> Result<(), CueError> {
        if index >= self.list.len() {
            return Err(self.fail_at(None, CueError::PlayheadOutOfBounds(index), self.now_ms));
        }
        self.playhead = Some(index);
        Ok(())
    }

    /// Clears the playhead selection.
    pub fn clear_playhead(&mut self) {
        self.playhead = None;
    }

    /// Binds a cart slot to an existing cue.
    pub fn bind_cart(&mut self, slot: u32, cue_id: CueId) -> Result<(), CueError> {
        if !self.cues.contains_key(&cue_id) {
            return Err(self.fail_at(Some(cue_id), CueError::UnknownCue(cue_id), self.now_ms));
        }
        self.cart.insert(slot, cue_id);
        Ok(())
    }

    /// Returns the cue bound to a cart slot.
    pub fn cart_binding(&self, slot: u32) -> Option<CueId> {
        self.cart.get(&slot).copied()
    }

    /// Triggers the playhead cue and advances the playhead by one position.
    ///
    /// Automatic continuations never move this user-facing playhead.
    pub fn go(&mut self) -> Result<CueId, CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        let id = self
            .playhead_cue()
            .ok_or_else(|| self.fail_at(None, CueError::NoCueAtPlayhead, self.now_ms))?;
        self.start_internal(id, now_ms, None)?;
        self.settle_all(now_ms)?;
        if let Some(index) = self.playhead {
            self.playhead = (index + 1 < self.list.len()).then_some(index + 1);
        }
        Ok(id)
    }

    /// Starts the cue bound to a cart slot without changing the playhead.
    pub fn trigger_cart(&mut self, slot: u32) -> Result<CueId, CueError> {
        let id = self.cart_cue(slot)?;
        self.start(id)?;
        Ok(id)
    }

    /// Starts an inactive cart cue or stops its active cart cue.
    pub fn toggle_cart(&mut self, slot: u32) -> Result<CueId, CueError> {
        let id = self.cart_cue(slot)?;
        if self.runtimes.get(&id).is_some_and(Runtime::is_active) {
            self.stop(id)?;
        } else {
            self.start(id)?;
        }
        Ok(id)
    }

    /// Starts a cue immediately, including its configured pre-wait.
    pub fn start(&mut self, id: CueId) -> Result<(), CueError> {
        self.start_faded_internal(id, None)
    }

    /// Starts a cue and ramps volume and opacity from zero to normal values.
    ///
    /// The fade begins when the action enters `Running`, after pre-wait.
    pub fn start_faded(
        &mut self,
        id: CueId,
        duration_ms: u64,
        curve: FadeCurve,
    ) -> Result<(), CueError> {
        self.start_faded_internal(id, Some((duration_ms, curve)))
    }

    fn start_faded_internal(
        &mut self,
        id: CueId,
        fade: Option<(u64, FadeCurve)>,
    ) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        self.start_internal(id, now_ms, fade)?;
        self.settle_all(now_ms)
    }

    /// Stops a cue from any state and cancels its timers and fades.
    pub fn stop(&mut self, id: CueId) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        if !self.cues.contains_key(&id) {
            return Err(self.fail_at(Some(id), CueError::UnknownCue(id), now_ms));
        }
        self.stop_internal(id, now_ms);
        Ok(())
    }

    /// Fades volume and opacity to zero, then stops the cue.
    ///
    /// A zero-duration faded stop is equivalent to [`stop`](Self::stop).
    pub fn stop_faded(
        &mut self,
        id: CueId,
        duration_ms: u64,
        curve: FadeCurve,
    ) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        let state = self
            .state(id)
            .ok_or_else(|| self.fail_at(Some(id), CueError::UnknownCue(id), now_ms))?;
        if !matches!(
            state,
            CueState::PreWait | CueState::Running | CueState::PostWait
        ) {
            return Err(self.fail_at(
                Some(id),
                CueError::InvalidState { cue_id: id, state },
                now_ms,
            ));
        }
        if duration_ms == 0 {
            self.stop_internal(id, now_ms);
            return Ok(());
        }
        let (volume, opacity) = {
            let runtime = self.runtime(id);
            (runtime.volume, runtime.opacity)
        };
        self.runtime_mut(id).fade = Some(ActiveFade {
            started_ms: now_ms,
            duration_ms,
            curve,
            from_volume: volume,
            to_volume: 0.0,
            from_opacity: opacity,
            to_opacity: 0.0,
            purpose: FadePurpose::Stop,
        });
        self.update_fade(id, now_ms);
        Ok(())
    }

    /// Pauses pre-wait, action, post-wait, continuation, and fade timing.
    pub fn pause(&mut self, id: CueId) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        self.pause_internal(id, now_ms)
    }

    /// Fades volume and opacity to zero, then pauses the cue.
    ///
    /// Cue timing continues during the fade and freezes when it finishes.
    pub fn pause_faded(
        &mut self,
        id: CueId,
        duration_ms: u64,
        curve: FadeCurve,
    ) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        let state = self
            .state(id)
            .ok_or_else(|| self.fail_at(Some(id), CueError::UnknownCue(id), now_ms))?;
        if !matches!(
            state,
            CueState::PreWait | CueState::Running | CueState::PostWait
        ) {
            return Err(self.fail_at(
                Some(id),
                CueError::InvalidState { cue_id: id, state },
                now_ms,
            ));
        }
        if duration_ms == 0 {
            return self.pause_internal(id, now_ms);
        }
        let (volume, opacity) = {
            let runtime = self.runtime(id);
            (runtime.volume, runtime.opacity)
        };
        self.runtime_mut(id).fade = Some(ActiveFade {
            started_ms: now_ms,
            duration_ms,
            curve,
            from_volume: volume,
            to_volume: 0.0,
            from_opacity: opacity,
            to_opacity: 0.0,
            purpose: FadePurpose::Pause,
        });
        self.update_fade(id, now_ms);
        Ok(())
    }

    /// Resumes a paused cue with its exact remaining phase and fade times.
    pub fn resume(&mut self, id: CueId) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        self.resume_internal(id, now_ms)?;
        self.settle_all(now_ms)
    }

    /// Resumes a paused cue and fades its parameters from zero to normal values.
    pub fn resume_faded(
        &mut self,
        id: CueId,
        duration_ms: u64,
        curve: FadeCurve,
    ) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        self.resume_internal(id, now_ms)?;
        let (volume, opacity) = {
            let cue = &self.cues[&id];
            (cue.volume(), cue.opacity())
        };
        {
            let runtime = self.runtime_mut(id);
            if duration_ms == 0 {
                runtime.volume = volume;
                runtime.opacity = opacity;
            } else {
                runtime.volume = 0.0;
                runtime.opacity = 0.0;
                runtime.fade = Some(ActiveFade {
                    started_ms: now_ms,
                    duration_ms,
                    curve,
                    from_volume: 0.0,
                    to_volume: volume,
                    from_opacity: 0.0,
                    to_opacity: opacity,
                    purpose: FadePurpose::Start,
                });
            }
        }
        self.settle_all(now_ms)
    }

    /// Reports that an audio or video action finished in the media adapter.
    ///
    /// This completes the action immediately, starts post-wait, and fires an
    /// auto-follow successor when configured.
    pub fn notify_media_completed(&mut self, id: CueId) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)?;
        let valid = self.cues.get(&id).is_some_and(|cue| cue.kind().is_media())
            && self.state(id) == Some(CueState::Running);
        if !valid {
            return Err(self.fail_at(Some(id), CueError::InvalidMediaCompletion(id), now_ms));
        }
        self.complete_action(id, now_ms)?;
        self.settle_all(now_ms)
    }

    /// Advances every cue using the clock's current timestamp.
    pub fn tick_now(&mut self) -> Result<(), CueError> {
        let now_ms = self.clock.now_ms();
        self.tick(now_ms)
    }

    /// Advances every cue to an explicit monotonic timestamp.
    ///
    /// Large time steps preserve transition timestamps, so zero-duration chains
    /// and follow-on cues settle deterministically at their actual deadlines.
    pub fn tick(&mut self, now_ms: u64) -> Result<(), CueError> {
        if now_ms < self.now_ms {
            let error = CueError::NonMonotonicClock {
                previous_ms: self.now_ms,
                received_ms: now_ms,
            };
            return Err(self.fail_at(None, error, self.now_ms));
        }
        self.now_ms = now_ms;
        self.settle_all(now_ms)
    }

    /// Returns the latest timestamp observed by the engine.
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Removes and returns all queued events in emission order.
    pub fn drain_events(&mut self) -> Vec<EngineEvent> {
        self.events.drain(..).collect()
    }

    /// Returns the next queued event without removing later events.
    pub fn pop_event(&mut self) -> Option<EngineEvent> {
        self.events.pop_front()
    }

    /// Returns the number of queued events.
    pub fn pending_event_count(&self) -> usize {
        self.events.len()
    }

    fn cart_cue(&mut self, slot: u32) -> Result<CueId, CueError> {
        let id = self
            .cart
            .get(&slot)
            .copied()
            .ok_or_else(|| self.fail_at(None, CueError::UnboundCartSlot(slot), self.now_ms))?;
        if !self.cues.contains_key(&id) {
            return Err(self.fail_at(Some(id), CueError::UnknownCartCue(slot), self.now_ms));
        }
        Ok(id)
    }

    fn runtime(&self, id: CueId) -> &Runtime {
        // Every definition has exactly one runtime; callers validate IDs first.
        &self.runtimes[&id]
    }

    fn runtime_mut(&mut self, id: CueId) -> &mut Runtime {
        // Every definition has exactly one runtime; callers validate IDs first.
        self.runtimes.get_mut(&id).expect("cue runtime missing")
    }

    fn fail_at(&mut self, cue_id: Option<CueId>, error: CueError, at_ms: u64) -> CueError {
        self.events.push_back(EngineEvent::Error {
            cue_id,
            error: error.clone(),
            at_ms,
        });
        error
    }

    fn emit_state(&mut self, id: CueId, state: CueState, at_ms: u64) {
        self.events.push_back(EngineEvent::CueStateChanged {
            cue_id: id,
            state,
            at_ms,
        });
    }

    fn emit_media(&mut self, id: CueId, action: MediaAction, at_ms: u64) {
        if self.cues.get(&id).is_some_and(|cue| cue.kind().is_media()) {
            self.events.push_back(EngineEvent::MediaActionRequested {
                cue_id: id,
                action,
                at_ms,
            });
        }
    }

    fn start_internal(
        &mut self,
        id: CueId,
        now_ms: u64,
        fade: Option<(u64, FadeCurve)>,
    ) -> Result<(), CueError> {
        let cue = match self.cues.get(&id) {
            Some(cue) => cue.clone(),
            None => return Err(self.fail_at(Some(id), CueError::UnknownCue(id), now_ms)),
        };
        let state = self.runtime(id).state;
        if self.runtime(id).is_active() {
            return Err(self.fail_at(
                Some(id),
                CueError::InvalidState { cue_id: id, state },
                now_ms,
            ));
        }
        if matches!(cue.kind(), CueKind::Wait | CueKind::Fade { .. }) && cue.duration_ms().is_none()
        {
            return Err(self.fail_at(Some(id), CueError::DurationRequired(id), now_ms));
        }
        if let Err(error) = self.validate_group_graph(id) {
            return Err(self.fail_at(Some(id), error, now_ms));
        }

        let (volume, opacity) = if fade.is_some() {
            (0.0, 0.0)
        } else {
            (cue.volume(), cue.opacity())
        };
        self.runtimes.insert(
            id,
            Runtime {
                state: CueState::PreWait,
                phase: Some(Phase::PreWait),
                phase_end_ms: Some(now_ms.saturating_add(cue.pre_wait_ms())),
                pending_start_fade: fade,
                volume,
                opacity,
                ..Runtime::default()
            },
        );
        self.emit_state(id, CueState::PreWait, now_ms);
        Ok(())
    }

    fn settle_all(&mut self, now_ms: u64) -> Result<(), CueError> {
        loop {
            let ids: Vec<_> = self.cues.keys().copied().collect();
            let mut changed = false;
            for id in ids {
                changed |= self.advance_cue(id, now_ms)?;
            }
            if !changed {
                break;
            }
        }
        let ids: Vec<_> = self.cues.keys().copied().collect();
        for id in ids {
            self.update_fade(id, now_ms);
        }
        Ok(())
    }

    fn advance_cue(&mut self, id: CueId, now_ms: u64) -> Result<bool, CueError> {
        let mut changed = false;
        loop {
            let runtime = self.runtime(id);
            if matches!(
                runtime.state,
                CueState::Stopped | CueState::Paused | CueState::Completed | CueState::Error
            ) {
                break;
            }
            let continue_due = runtime.continue_due_ms;
            let fade_due = runtime.fade.as_ref().map(ActiveFade::end_ms);
            let phase_due = runtime.phase_end_ms;
            let next_due = [continue_due, fade_due, phase_due]
                .into_iter()
                .flatten()
                .min();
            let Some(due_ms) = next_due else {
                break;
            };
            if due_ms > now_ms {
                break;
            }

            if continue_due == Some(due_ms) {
                self.runtime_mut(id).continue_due_ms = None;
                if let Some(next) = self.next_list_cue(id) {
                    self.start_internal(next, due_ms, None)?;
                }
            } else if fade_due == Some(due_ms) {
                self.finish_fade(id, due_ms)?;
            } else {
                match runtime.phase {
                    Some(Phase::PreWait) => self.enter_running(id, due_ms)?,
                    Some(Phase::Running) => self.complete_action(id, due_ms)?,
                    Some(Phase::PostWait) => self.complete_cue(id, due_ms)?,
                    None => break,
                }
            }
            changed = true;
        }
        Ok(changed)
    }

    fn enter_running(&mut self, id: CueId, at_ms: u64) -> Result<(), CueError> {
        let cue = self.cues[&id].clone();
        let pending_fade = self.runtime_mut(id).pending_start_fade.take();
        let continuation_due = (cue.auto_continue() == AutoContinue::Continue)
            .then(|| at_ms.saturating_add(cue.post_wait_ms()));
        let is_group = matches!(cue.kind(), CueKind::Group { .. });
        {
            let runtime = self.runtime_mut(id);
            runtime.state = CueState::Running;
            runtime.phase = Some(Phase::Running);
            runtime.phase_end_ms = if is_group {
                None
            } else {
                cue.duration_ms()
                    .map(|duration| at_ms.saturating_add(duration))
            };
            runtime.continue_due_ms = continuation_due;
            runtime.volume = if pending_fade.is_some() {
                0.0
            } else {
                cue.volume()
            };
            runtime.opacity = if pending_fade.is_some() {
                0.0
            } else {
                cue.opacity()
            };
            if let Some((duration_ms, curve)) = pending_fade {
                if duration_ms > 0 {
                    runtime.fade = Some(ActiveFade {
                        started_ms: at_ms,
                        duration_ms,
                        curve,
                        from_volume: 0.0,
                        to_volume: cue.volume(),
                        from_opacity: 0.0,
                        to_opacity: cue.opacity(),
                        purpose: FadePurpose::Start,
                    });
                } else {
                    runtime.volume = cue.volume();
                    runtime.opacity = cue.opacity();
                }
            }
        }
        self.emit_state(id, CueState::Running, at_ms);
        self.emit_media(id, MediaAction::Play, at_ms);

        match cue.kind() {
            CueKind::Fade {
                target,
                volume,
                opacity,
                curve,
            } => {
                let target = *target;
                let curve = *curve;
                if self.state(target) != Some(CueState::Running) {
                    self.set_error_state(id, at_ms);
                    let error = CueError::TargetNotRunning(target);
                    return Err(self.fail_at(Some(id), error, at_ms));
                }
                let (from_volume, from_opacity) = {
                    let target_runtime = self.runtime(target);
                    (target_runtime.volume, target_runtime.opacity)
                };
                let duration_ms = cue.duration_ms().unwrap_or(0);
                self.runtime_mut(target).fade = Some(ActiveFade {
                    started_ms: at_ms,
                    duration_ms,
                    curve,
                    from_volume,
                    to_volume: volume.map(sanitize_parameter).unwrap_or(from_volume),
                    from_opacity,
                    to_opacity: opacity.map(sanitize_parameter).unwrap_or(from_opacity),
                    purpose: FadePurpose::Parameters,
                });
            }
            CueKind::Group { children } => {
                self.runtime_mut(id).group = Some(GroupProgress {
                    children: children.clone(),
                    next_index: 0,
                    waiting_on: None,
                });
                if let Err(error) = self.advance_group(id, at_ms) {
                    self.set_error_state(id, at_ms);
                    return Err(error);
                }
            }
            CueKind::Audio { .. } | CueKind::Video { .. } | CueKind::Wait => {}
        }
        Ok(())
    }

    fn complete_action(&mut self, id: CueId, at_ms: u64) -> Result<(), CueError> {
        if self.state(id) != Some(CueState::Running) {
            return Ok(());
        }
        let (post_wait_ms, auto_continue) = {
            let cue = &self.cues[&id];
            (cue.post_wait_ms(), cue.auto_continue())
        };
        {
            let runtime = self.runtime_mut(id);
            runtime.state = CueState::PostWait;
            runtime.phase = Some(Phase::PostWait);
            runtime.phase_end_ms = Some(at_ms.saturating_add(post_wait_ms));
            runtime.fade = None;
            runtime.pending_start_fade = None;
        }
        self.emit_state(id, CueState::PostWait, at_ms);
        if auto_continue == AutoContinue::Follow {
            if let Some(next) = self.next_list_cue(id) {
                self.start_internal(next, at_ms, None)?;
            }
        }
        Ok(())
    }

    fn complete_cue(&mut self, id: CueId, at_ms: u64) -> Result<(), CueError> {
        let (volume, opacity) = {
            let runtime = self.runtime(id);
            (runtime.volume, runtime.opacity)
        };
        self.runtimes.insert(
            id,
            Runtime {
                state: CueState::Completed,
                volume,
                opacity,
                ..Runtime::default()
            },
        );
        self.emit_state(id, CueState::Completed, at_ms);
        self.events
            .push_back(EngineEvent::CueCompleted { cue_id: id, at_ms });

        let parents: Vec<_> = self
            .runtimes
            .iter()
            .filter_map(|(parent_id, runtime)| {
                (runtime.group.as_ref().and_then(|group| group.waiting_on) == Some(id))
                    .then_some(*parent_id)
            })
            .collect();
        for parent in parents {
            if let Some(group) = self.runtime_mut(parent).group.as_mut() {
                group.waiting_on = None;
            }
            self.advance_group(parent, at_ms)?;
        }
        Ok(())
    }

    fn advance_group(&mut self, id: CueId, at_ms: u64) -> Result<(), CueError> {
        if self.state(id) != Some(CueState::Running) {
            return Ok(());
        }
        let next = {
            let group = self.runtime_mut(id).group.as_mut();
            match group {
                Some(group) if group.next_index < group.children.len() => {
                    let child = group.children[group.next_index];
                    group.next_index += 1;
                    group.waiting_on = Some(child);
                    Some(child)
                }
                _ => None,
            }
        };
        if let Some(child) = next {
            self.start_internal(child, at_ms, None)?;
        } else {
            self.complete_action(id, at_ms)?;
        }
        Ok(())
    }

    fn finish_fade(&mut self, id: CueId, at_ms: u64) -> Result<(), CueError> {
        let Some(fade) = self.runtime_mut(id).fade.take() else {
            return Ok(());
        };
        {
            let runtime = self.runtime_mut(id);
            runtime.volume = fade.to_volume;
            runtime.opacity = fade.to_opacity;
        }
        self.events.push_back(EngineEvent::FadeUpdated {
            cue_id: id,
            volume: fade.to_volume,
            opacity: fade.to_opacity,
            at_ms,
        });
        match fade.purpose {
            FadePurpose::Stop => self.stop_internal(id, at_ms),
            FadePurpose::Pause => self.pause_internal(id, at_ms)?,
            FadePurpose::Start | FadePurpose::Parameters => {}
        }
        Ok(())
    }

    fn update_fade(&mut self, id: CueId, now_ms: u64) {
        let values = {
            let runtime = self.runtime(id);
            if runtime.state == CueState::Paused {
                return;
            }
            runtime.fade.as_ref().map(|fade| fade.values(now_ms))
        };
        if let Some((volume, opacity)) = values {
            let runtime = self.runtime_mut(id);
            runtime.volume = volume;
            runtime.opacity = opacity;
            self.events.push_back(EngineEvent::FadeUpdated {
                cue_id: id,
                volume,
                opacity,
                at_ms: now_ms,
            });
        }
    }

    fn pause_internal(&mut self, id: CueId, now_ms: u64) -> Result<(), CueError> {
        let state = self
            .state(id)
            .ok_or_else(|| self.fail_at(Some(id), CueError::UnknownCue(id), now_ms))?;
        if !matches!(
            state,
            CueState::PreWait | CueState::Running | CueState::PostWait
        ) {
            return Err(self.fail_at(
                Some(id),
                CueError::InvalidState { cue_id: id, state },
                now_ms,
            ));
        }
        let child = {
            let runtime = self.runtime_mut(id);
            runtime.paused_phase_remaining_ms = runtime.phase_remaining_ms(now_ms);
            runtime.paused_continue_remaining_ms = runtime
                .continue_due_ms
                .map(|due_ms| due_ms.saturating_sub(now_ms));
            runtime.paused_fade_elapsed_ms =
                runtime.fade.as_ref().map(|fade| fade.elapsed_ms(now_ms));
            runtime.state = CueState::Paused;
            runtime.group.as_ref().and_then(|group| group.waiting_on)
        };
        self.emit_state(id, CueState::Paused, now_ms);
        if self.runtime(id).phase == Some(Phase::Running) {
            self.emit_media(id, MediaAction::Pause, now_ms);
        }
        if let Some(child) = child {
            if self.runtimes.get(&child).is_some_and(Runtime::is_active) {
                self.pause_internal(child, now_ms)?;
            }
        }
        Ok(())
    }

    fn resume_internal(&mut self, id: CueId, now_ms: u64) -> Result<(), CueError> {
        if self.state(id) != Some(CueState::Paused) {
            let state = self.state(id);
            return match state {
                None => Err(self.fail_at(Some(id), CueError::UnknownCue(id), now_ms)),
                Some(state) => Err(self.fail_at(
                    Some(id),
                    CueError::InvalidState { cue_id: id, state },
                    now_ms,
                )),
            };
        }
        let (phase, child) = {
            let runtime = self.runtime_mut(id);
            let phase = runtime.phase.unwrap_or(Phase::Running);
            runtime.phase_end_ms = runtime
                .paused_phase_remaining_ms
                .map(|remaining| now_ms.saturating_add(remaining));
            runtime.continue_due_ms = runtime
                .paused_continue_remaining_ms
                .map(|remaining| now_ms.saturating_add(remaining));
            if let (Some(fade), Some(elapsed)) =
                (runtime.fade.as_mut(), runtime.paused_fade_elapsed_ms)
            {
                fade.started_ms = now_ms.saturating_sub(elapsed);
            }
            runtime.paused_phase_remaining_ms = None;
            runtime.paused_continue_remaining_ms = None;
            runtime.paused_fade_elapsed_ms = None;
            runtime.state = phase.state();
            let child = runtime.group.as_ref().and_then(|group| group.waiting_on);
            (phase, child)
        };
        self.emit_state(id, phase.state(), now_ms);
        if phase == Phase::Running {
            self.emit_media(id, MediaAction::Resume, now_ms);
        }
        if let Some(child) = child {
            if self.state(child) == Some(CueState::Paused) {
                self.resume_internal(child, now_ms)?;
            }
        }
        Ok(())
    }

    fn stop_internal(&mut self, id: CueId, at_ms: u64) {
        let old_runtime = self.runtime(id).clone();
        if old_runtime.state == CueState::Stopped {
            return;
        }
        if let Some(child) = old_runtime.group.and_then(|group| group.waiting_on) {
            if self.runtimes.get(&child).is_some_and(Runtime::is_active) {
                self.stop_internal(child, at_ms);
            }
        }
        self.runtimes.insert(
            id,
            Runtime {
                state: CueState::Stopped,
                volume: old_runtime.volume,
                opacity: old_runtime.opacity,
                ..Runtime::default()
            },
        );
        self.emit_state(id, CueState::Stopped, at_ms);
        let media_was_running = old_runtime.state == CueState::Running
            || (old_runtime.state == CueState::Paused && old_runtime.phase == Some(Phase::Running));
        if media_was_running {
            self.emit_media(id, MediaAction::Stop, at_ms);
        }
    }

    fn set_error_state(&mut self, id: CueId, at_ms: u64) {
        let (volume, opacity) = {
            let runtime = self.runtime(id);
            (runtime.volume, runtime.opacity)
        };
        self.runtimes.insert(
            id,
            Runtime {
                state: CueState::Error,
                volume,
                opacity,
                ..Runtime::default()
            },
        );
        self.emit_state(id, CueState::Error, at_ms);
    }

    fn next_list_cue(&self, id: CueId) -> Option<CueId> {
        let index = self.list.iter().position(|candidate| *candidate == id)?;
        self.list.get(index + 1).copied()
    }

    fn validate_group_graph(&self, id: CueId) -> Result<(), CueError> {
        fn visit(
            engine: &Engine,
            id: CueId,
            visiting: &mut BTreeSet<CueId>,
            visited: &mut BTreeSet<CueId>,
        ) -> Result<(), CueError> {
            if visiting.contains(&id) {
                return Err(CueError::GroupCycle(id));
            }
            if visited.contains(&id) {
                return Ok(());
            }
            visiting.insert(id);
            if let Some(CueKind::Group { children }) = engine.cues.get(&id).map(Cue::kind) {
                for child in children {
                    visit(engine, *child, visiting, visited)?;
                }
            }
            visiting.remove(&id);
            visited.insert(id);
            Ok(())
        }

        visit(self, id, &mut BTreeSet::new(), &mut BTreeSet::new())
    }
}

fn sanitize_parameter(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

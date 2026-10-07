use super::*;
fn economy(policy: PresentationPolicy) -> Machine {
    let mut m = Machine::new(policy);
    m.snapshot.state = policy.control_surface();
    m.snapshot.epoch = 1;
    m.snapshot.ui_guard = false;
    m
}
#[test]
fn manual_preferences_never_schedule_and_auto_never_selects_presence() {
    for policy in [PresentationPolicy::Economy, PresentationPolicy::Presence, PresentationPolicy::Headless] {
        let mut m = economy(policy);
        assert!(m.focus(1, false, Instant::now()).is_none());
        assert!(m.pending.is_none());
    }
    for policy in [PresentationPolicy::Auto, PresentationPolicy::Headless, PresentationPolicy::Economy] { assert_eq!(policy.control_surface(), RuntimeState::Economy); }
    assert_eq!(PresentationPolicy::Presence.control_surface(), RuntimeState::Presence);
}
#[test]
fn full_hysteresis_focus_cancellation_stale_epoch_and_policy_tokens() {
    let now = Instant::now();
    let mut m = economy(PresentationPolicy::Auto);
    let token = m.focus(1, false, now).unwrap();
    assert!(!m.eligible(token, now + AUTO_HEADLESS_DELAY - Duration::from_millis(1), false, 1));
    assert!(m.eligible(token, now + AUTO_HEADLESS_DELAY, false, 1));
    assert!(m.focus(1, false, now).is_none()); // duplicate blur cannot add a timer
    m.focus(1, true, now);
    assert!(!m.eligible(token, now + AUTO_HEADLESS_DELAY, false, 1));
    let stale = m.focus(1, false, now).unwrap();
    m.invalidate(); m.snapshot.policy = PresentationPolicy::Presence;
    assert!(!m.eligible(stale, now + AUTO_HEADLESS_DELAY, false, 1));
    m.snapshot.policy = PresentationPolicy::Auto; m.snapshot.epoch = 2; m.snapshot.focused = true;
    assert!(m.focus(1, false, now).is_none()); // old window callback
    let fresh = m.focus(2, false, now + Duration::from_secs(10)).unwrap();
    assert!(!m.eligible(fresh, now + AUTO_HEADLESS_DELAY, false, 1));
    assert!(m.eligible(fresh, now + Duration::from_secs(40), false, 1));
}
#[test]
fn guards_attention_auxiliary_and_transitions_fail_closed() {
    let now = Instant::now(); let mut m = economy(PresentationPolicy::Auto);
    let token = m.focus(1, false, now).unwrap(); let later = now + AUTO_HEADLESS_DELAY;
    assert!(!m.eligible(token, later, true, 1));
    for windows in [0,2,3] { assert!(!m.eligible(token, later, false, windows)); }
    m.snapshot.ui_guard = true; assert!(!m.eligible(token, later, false, 1)); m.snapshot.ui_guard = false;
    for reason in [AttentionReason::ApprovalRequired, AttentionReason::UserInputRequired, AttentionReason::TaskFailed] {
        m.snapshot.attention = Some(reason); assert!(!m.eligible(token, later, false, 1));
    }
    m.snapshot.attention = None;
    m.snapshot.transitioning = true; assert!(!m.eligible(token, later, false, 1)); m.snapshot.transitioning = false;
    m.snapshot.quitting = true; m.invalidate(); assert!(!m.eligible(token, later, false, 1));
    assert!(m.focus(1, false, later).is_none());
}
#[test]
fn transient_close_preserves_preference_and_bounded_allowlisted_telemetry() {
    let mut m = economy(PresentationPolicy::Presence);
    for _ in 0..100 {
        m.record(RuntimeState::Headless, Reason::ManualClose);
        assert_eq!(m.snapshot.policy, PresentationPolicy::Presence);
        m.record(m.snapshot.policy.control_surface(), Reason::ExplicitActivation);
    }
    assert_eq!(m.snapshot.history.len(), HISTORY_LIMIT);
    let json = serde_json::to_value(&m.snapshot.history).unwrap();
    for item in json.as_array().unwrap() {
        assert_eq!(item.as_object().unwrap().keys().cloned().collect::<Vec<_>>(), vec!["from","policy","reason","timestamp","to"]);
    }
    assert!(serde_json::from_str::<AttentionReason>("\"private conversation\"").is_err());
    assert!(serde_json::from_str::<PresentationPolicy>("\"unknown\"").is_err());
}
#[test]
fn migration_preserves_legacy_opt_in_new_default_and_reopen() {
    for mode in ["economy", "presence"] {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/017_economy_shell.sql")).unwrap();
        conn.execute("UPDATE shell_settings SET presentation_mode=?1", [mode]).unwrap();
        conn.execute_batch(include_str!("../migrations/018_presentation_policy.sql")).unwrap();
        let settings = shell_settings::load(&conn).unwrap();
        assert_eq!(serde_json::to_value(settings.presentation_policy).unwrap(), mode);
    }
    let dir = std::env::temp_dir().join(format!("narys-adaptive-test-{}", std::process::id()));
    let db = Database::new(dir.clone());
    let conn = db.open().unwrap();
    assert_eq!(shell_settings::load(&conn).unwrap().presentation_policy, PresentationPolicy::Economy);
    let cognition = crate::cognition::policy::load(&conn, crate::cognition::policy::CognitiveRole::Conversation).unwrap();
    for policy in [PresentationPolicy::Presence,PresentationPolicy::Auto,PresentationPolicy::Headless,PresentationPolicy::Economy] {
        shell_settings::save_policy(&conn, policy).unwrap();
        assert_eq!(shell_settings::load(&db.open().unwrap()).unwrap().presentation_policy, policy);
        assert_eq!(crate::cognition::policy::load(&conn, crate::cognition::policy::CognitiveRole::Conversation).unwrap(), cognition);
    }
    drop(conn); std::fs::remove_dir_all(dir).unwrap();
}

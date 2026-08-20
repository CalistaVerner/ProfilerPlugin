use super::*;

#[test]
fn duration_parser_reads_first_millisecond_pair_without_token_vector() {
    assert_eq!(
        parse_first_ms_from_text("load took 12.5 ms total"),
        Some(12.5)
    );
    assert_eq!(
        parse_first_ms_from_text("gpu 7 milliseconds blocked"),
        Some(7.0)
    );
    assert_eq!(parse_first_ms_from_text("no duration"), None);
}

#[test]
fn classification_join_is_lowercase_and_space_stable() {
    assert_eq!(
        lower_join(&[Some("RUNNING"), None, Some("GPU Wait")]),
        "running gpu wait"
    );
}

#[test]
fn breakdown_parser_keeps_valid_non_negative_parts() {
    assert_eq!(
        parse_breakdown_parts("decode=1.5ms upload=2ms ignored broken=x"),
        vec![("decode".to_owned(), 1.5), ("upload".to_owned(), 2.0)]
    );
}

#[test]
fn bool_metadata_parser_is_case_insensitive_without_normalization() {
    let value = json!({"enabled": "YeS", "disabled": "OFF"});
    assert_eq!(first_value_bool(&value, &["/enabled"]), Some(true));
    assert_eq!(first_value_bool(&value, &["/disabled"]), Some(false));
}

#[test]
fn completed_ring_preserves_slow_outlier_under_healthy_flood() {
    let mut cfg = ProfilerConfig::default();
    cfg.diagnostics.max_recent_jobs = 3;
    cfg.diagnostics.slow_job_warn_ms = 16.67;
    let runtime = ProfilerRuntime::new(cfg, None);
    let mut state = ProfilerState::new();

    let make = |id: &str, elapsed_ms: f64| JobRecord {
        id: id.to_owned(),
        name: id.to_owned(),
        category: "test".to_owned(),
        source: "test".to_owned(),
        lane: "test".to_owned(),
        priority: "normal".to_owned(),
        dependency_group: "test".to_owned(),
        frame_id: None,
        status: "completed".to_owned(),
        detail: String::new(),
        scheduled: false,
        blocked: false,
        polling: false,
        waited_on_gpu: false,
        stayed_async: false,
        exceeded_frame_budget: false,
        frame_budget_ms: None,
        gpu_wait_ms: None,
        wait_reason: None,
        async_mode: None,
        started_unix_ms: 0,
        ended_unix_ms: Some(1),
        elapsed_ms: Some(elapsed_ms),
        budget_ms: 16.67,
        load: Some(elapsed_ms / 16.67),
        progress: None,
        payload_bytes: None,
        output_bytes: None,
        error: None,
        metadata: json!({}),
    };

    runtime.complete_job_locked(&mut state, make("slow", 48.0));
    runtime.complete_job_locked(&mut state, make("normal-1", 0.2));
    runtime.complete_job_locked(&mut state, make("normal-2", 0.2));
    runtime.complete_job_locked(&mut state, make("normal-3", 0.2));
    runtime.complete_job_locked(&mut state, make("normal-4", 0.2));

    assert_eq!(state.completed.len(), 3);
    assert!(state.completed.iter().any(|job| job.id == "slow"));
}

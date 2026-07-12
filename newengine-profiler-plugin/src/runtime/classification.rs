use super::*;

pub(super) fn refresh_record_classification(record: &mut JobRecord) {
    if let Some(v) = first_value_str(
        &record.metadata,
        &["/lane", "/metadata/lane", "/event/lane"],
    ) {
        record.lane = sanitize_non_empty(Some(v.as_str()), record.lane.as_str());
    }
    if let Some(v) = first_value_str(
        &record.metadata,
        &["/priority", "/metadata/priority", "/event/priority"],
    ) {
        record.priority = sanitize_non_empty(Some(v.as_str()), record.priority.as_str());
    }
    if let Some(v) = first_value_str(
        &record.metadata,
        &[
            "/dependency_group",
            "/dependencyGroup",
            "/metadata/dependency_group",
            "/metadata/dependencyGroup",
        ],
    ) {
        record.dependency_group =
            sanitize_non_empty(Some(v.as_str()), record.dependency_group.as_str());
    }
    record.frame_id = record.frame_id.or_else(|| {
        first_value_u64(
            &record.metadata,
            &[
                "/frame_id",
                "/frame",
                "/frame_index",
                "/metadata/frame_id",
                "/metadata/frame",
                "/metadata/frame_index",
            ],
        )
    });
    record.frame_budget_ms = record.frame_budget_ms.or_else(|| {
        first_value_f64(
            &record.metadata,
            &[
                "/frame_budget_ms",
                "/budget/frame_ms",
                "/metadata/frame_budget_ms",
                "/metadata/budget/frame_ms",
            ],
        )
    });
    record.gpu_wait_ms = record.gpu_wait_ms.or_else(|| {
        first_value_f64(
            &record.metadata,
            &[
                "/gpu_wait_ms",
                "/waited_gpu_ms",
                "/metadata/gpu_wait_ms",
                "/metadata/waited_gpu_ms",
            ],
        )
    });
    if record
        .wait_reason
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        record.wait_reason = first_value_str(
            &record.metadata,
            &[
                "/wait_reason",
                "/blocked_reason",
                "/block_reason",
                "/metadata/wait_reason",
                "/metadata/blocked_reason",
                "/metadata/block_reason",
            ],
        );
    }
    if record
        .async_mode
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        record.async_mode = first_value_str(
            &record.metadata,
            &[
                "/async_mode",
                "/scheduling_mode",
                "/metadata/async_mode",
                "/metadata/scheduling_mode",
            ],
        );
    }

    let metadata_phase = first_value_str(
        &record.metadata,
        &[
            "/phase",
            "/state_label",
            "/status",
            "/metadata/phase",
            "/metadata/state_label",
            "/metadata/status",
        ],
    );
    let phase = lower_join(&[
        Some(record.status.as_str()),
        Some(record.detail.as_str()),
        metadata_phase.as_deref(),
    ]);
    let category = record.category.to_ascii_lowercase();
    let wait_reason = record
        .wait_reason
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let async_mode = record
        .async_mode
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    record.scheduled |= contains_any(&phase, &["scheduled", "queued", "queue", "accepted"]);
    record.blocked |= contains_any(&phase, &["blocked", "waiting", "stalled"])
        || contains_any(
            &wait_reason,
            &["blocked", "waiting", "dependency", "residency", "barrier"],
        );
    record.polling |=
        contains_any(&phase, &["poll", "polling"]) || contains_any(&category, &["poll"]);
    record.waited_on_gpu |= record.gpu_wait_ms.unwrap_or_default() > 0.0
        || contains_any(
            &wait_reason,
            &["gpu", "fence", "present", "upload", "queue"],
        );
    record.stayed_async |= first_value_bool(
        &record.metadata,
        &[
            "/stayed_async",
            "/async",
            "/metadata/stayed_async",
            "/metadata/async",
        ],
    )
    .unwrap_or(false)
        || contains_any(
            &async_mode,
            &["async", "engine_threading", "job", "ticket", "poll"],
        )
        || contains_any(
            &category,
            &[
                "shader.compile",
                "asset.decode",
                "texture.upload",
                "streaming",
                "residency",
                "renderprep",
                "render-prep",
            ],
        );

    if let (Some(elapsed), Some(frame_budget)) = (record.elapsed_ms, record.frame_budget_ms) {
        record.exceeded_frame_budget |= elapsed > frame_budget.max(0.001);
    }
    record.exceeded_frame_budget |= first_value_bool(
        &record.metadata,
        &[
            "/exceeded_frame_budget",
            "/frame_budget_exceeded",
            "/metadata/exceeded_frame_budget",
            "/metadata/frame_budget_exceeded",
        ],
    )
    .unwrap_or(false);
}

pub(super) fn first_value_str(value: &Value, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .filter_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .next()
}

pub(super) fn first_value_u64(value: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| {
        value.pointer(path).and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str()?.trim().parse::<u64>().ok())
        })
    })
}

pub(super) fn first_value_f64(value: &Value, paths: &[&str]) -> Option<f64> {
    paths
        .iter()
        .find_map(|path| value.pointer(path).and_then(value_to_f64_ms))
}

pub(super) fn first_value_bool(value: &Value, paths: &[&str]) -> Option<bool> {
    paths.iter().find_map(|path| {
        value.pointer(path).and_then(|v| {
            v.as_bool().or_else(|| {
                let raw = v.as_str()?.trim();
                if ["1", "true", "yes", "y", "on"]
                    .iter()
                    .any(|candidate| raw.eq_ignore_ascii_case(candidate))
                {
                    Some(true)
                } else if ["0", "false", "no", "n", "off"]
                    .iter()
                    .any(|candidate| raw.eq_ignore_ascii_case(candidate))
                {
                    Some(false)
                } else {
                    None
                }
            })
        })
    })
}

pub(super) fn lower_join(parts: &[Option<&str>]) -> String {
    let capacity = parts
        .iter()
        .filter_map(|part| *part)
        .map(str::len)
        .sum::<usize>()
        .saturating_add(parts.len().saturating_sub(1));
    let mut joined = String::with_capacity(capacity);
    for part in parts.iter().filter_map(|part| *part) {
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.extend(part.chars().flat_map(char::to_lowercase));
    }
    joined
}

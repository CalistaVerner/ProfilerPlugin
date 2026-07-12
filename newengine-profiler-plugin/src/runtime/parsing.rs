use super::*;

pub(super) fn event_elapsed_ms(value: &Value) -> Option<f64> {
    const PATHS: &[&str] = &[
        "/elapsed_ms",
        "/duration_ms",
        "/total_ms",
        "/metadata/elapsed_ms",
        "/metadata/duration_ms",
        "/metadata/total_ms",
    ];
    for path in PATHS {
        if let Some(ms) = value.pointer(path).and_then(value_to_f64_ms) {
            return Some(ms.max(0.0));
        }
    }
    value
        .get("detail")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .and_then(parse_first_ms_from_text)
        .map(|ms| ms.max(0.0))
}

pub(super) fn value_to_f64_ms(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|text| text.trim().parse::<f64>().ok())
    })
}

pub(super) fn parse_first_ms_from_text(text: &str) -> Option<f64> {
    let mut previous: Option<&str> = None;
    for token in text.split_whitespace() {
        let unit = token.trim_matches(|character: char| !character.is_ascii_alphabetic());
        if ["ms", "msec", "millisecond", "milliseconds"]
            .iter()
            .any(|candidate| unit.eq_ignore_ascii_case(candidate))
        {
            let number = previous?.trim_matches(|character: char| {
                !(character.is_ascii_digit() || character == '.' || character == '-')
            });
            if let Ok(value) = number.parse::<f64>() {
                return Some(value);
            }
        }
        previous = Some(token);
    }
    None
}

pub(super) fn parse_breakdown_parts(breakdown: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for token in breakdown.split_whitespace() {
        let Some((name, raw_ms)) = token.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let raw_ms = raw_ms.trim().strip_suffix("ms").unwrap_or(raw_ms.trim());
        let Ok(elapsed_ms) = raw_ms.parse::<f64>() else {
            continue;
        };
        out.push((name.to_owned(), elapsed_ms.max(0.0)));
    }
    out
}

pub(super) fn engine_job_envelope_to_profiler_event(envelope: Value) -> Value {
    let Some(mut event) = envelope.get("event").cloned() else {
        return envelope;
    };
    let Value::Object(event_obj) = &mut event else {
        return event;
    };

    let envelope_meta = json!({
        "schema": envelope.get("schema").cloned(),
        "authority": envelope.get("authority").cloned(),
        "executor": envelope.get("executor").cloned(),
        "semantic_owner": envelope.get("semantic_owner").cloned(),
    });
    event_obj.insert("job_envelope".to_owned(), envelope_meta);
    if event_obj.get("owner").is_none() {
        if let Some(owner) = envelope.get("semantic_owner").cloned() {
            event_obj.insert("owner".to_owned(), owner);
        }
    }
    event
}

pub(super) fn is_high_frequency_zero_cost_event(
    category: &str,
    source: &str,
    name: &str,
    elapsed_ms: Option<f64>,
) -> bool {
    if elapsed_ms.unwrap_or(0.0) > 0.0 {
        return false;
    }
    category.eq_ignore_ascii_case("event")
        && ["raw-device", "event_bus", "cursor"]
            .iter()
            .any(|candidate| source.eq_ignore_ascii_case(candidate))
        && (name
            .get(.."winit.mouse_".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("winit.mouse_"))
            || name.eq_ignore_ascii_case("winit.key")
            || name.eq_ignore_ascii_case("winit.text_char"))
}

pub(super) fn recently_completed_duplicate_terminal(state: &ProfilerState, id: &str) -> bool {
    state
        .completed
        .iter()
        .rev()
        .take(256)
        .any(|record| record.id == id)
}

pub(super) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

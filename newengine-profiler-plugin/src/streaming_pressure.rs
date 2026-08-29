use std::collections::VecDeque;

use serde::Serialize;
use serde_json::{json, Value};

use crate::records::JobRecord;

const STREAMING_SAMPLE_SCHEMA: &str = "newengine.assets.streaming.control_plane.profiler.v1";
const MAX_SERIES_SAMPLES: usize = 512;

#[derive(Debug, Clone, Serialize, PartialEq)]
struct StreamingPressureSample {
    frame_id: u64,
    elapsed_ms: f64,
    budget_bytes: u64,
    committed_bytes: u64,
    projected_resident_bytes: u64,
    pressure_ratio: f64,
    pending_assets: u64,
    pending_demands: u64,
    in_flight_assets: u64,
    in_flight_bytes: u64,
    over_budget_bytes: u64,
    deadline_misses: u64,
    total_deadline_misses: u64,
    provider_failures: u64,
    admissions_dispatched: u64,
    evictions_dispatched: u64,
    completed_loads: u64,
    average_load_latency_frames: f64,
    max_load_latency_frames: u64,
}

/// Build a streaming-specific profiler read-model from generic completed events.
///
/// The profiler remains a passive observer. It never feeds pressure back into the
/// scheduler and therefore cannot become a second policy/control loop.
pub(crate) fn build_streaming_pressure(jobs: &VecDeque<JobRecord>) -> Value {
    let mut samples = jobs.iter().filter_map(sample_from_job).collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.frame_id);
    if samples.len() > MAX_SERIES_SAMPLES {
        samples.drain(..samples.len() - MAX_SERIES_SAMPLES);
    }

    let Some(latest) = samples.last().cloned() else {
        return json!({
            "schema": "newengine.profiler.streaming_pressure.v1",
            "sample_count": 0,
            "latest": null,
            "series": []
        });
    };

    let peak_pressure_ratio = samples
        .iter()
        .map(|sample| sample.pressure_ratio)
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max);
    let peak_pending_assets = samples
        .iter()
        .map(|sample| sample.pending_assets)
        .max()
        .unwrap_or_default();
    let peak_over_budget_bytes = samples
        .iter()
        .map(|sample| sample.over_budget_bytes)
        .max()
        .unwrap_or_default();
    let peak_load_latency_frames = samples
        .iter()
        .map(|sample| sample.max_load_latency_frames)
        .max()
        .unwrap_or_default();
    let provider_failure_observations = samples
        .iter()
        .map(|sample| sample.provider_failures)
        .sum::<u64>();
    let completed_loads = samples
        .iter()
        .map(|sample| sample.completed_loads)
        .sum::<u64>();
    let total_deadline_misses = samples
        .iter()
        .map(|sample| sample.total_deadline_misses)
        .max()
        .unwrap_or_default();

    json!({
        "schema": "newengine.profiler.streaming_pressure.v1",
        "sample_count": samples.len(),
        "latest": latest,
        "summary": {
            "peak_pressure_ratio": peak_pressure_ratio,
            "peak_pending_assets": peak_pending_assets,
            "peak_over_budget_bytes": peak_over_budget_bytes,
            "total_deadline_misses": total_deadline_misses,
            "provider_failure_observations": provider_failure_observations,
            "completed_loads_observed": completed_loads,
            "peak_load_latency_frames": peak_load_latency_frames,
        },
        "series": samples,
    })
}

fn sample_from_job(job: &JobRecord) -> Option<StreamingPressureSample> {
    let metadata = &job.metadata;
    if metadata.get("schema").and_then(Value::as_str) != Some(STREAMING_SAMPLE_SCHEMA) {
        return None;
    }
    let streaming = metadata.get("streaming")?;
    let frame_id = job
        .frame_id
        .or_else(|| metadata.get("frame_id").and_then(Value::as_u64))?;

    Some(StreamingPressureSample {
        frame_id,
        elapsed_ms: finite_f64(job.elapsed_ms.unwrap_or_default()),
        budget_bytes: u64_field(streaming, "budget_bytes"),
        committed_bytes: u64_field(streaming, "committed_bytes"),
        projected_resident_bytes: u64_field(streaming, "projected_resident_bytes"),
        pressure_ratio: finite_f64(f64_field(streaming, "pressure_ratio")),
        pending_assets: u64_field(streaming, "pending_assets"),
        pending_demands: u64_field(streaming, "pending_demands"),
        in_flight_assets: u64_field(streaming, "in_flight_assets"),
        in_flight_bytes: u64_field(streaming, "in_flight_bytes"),
        over_budget_bytes: u64_field(streaming, "over_budget_bytes"),
        deadline_misses: u64_field(streaming, "deadline_misses"),
        total_deadline_misses: u64_field(streaming, "total_deadline_misses"),
        provider_failures: u64_field(streaming, "provider_failures"),
        admissions_dispatched: u64_field(streaming, "admissions_dispatched"),
        evictions_dispatched: u64_field(streaming, "evictions_dispatched"),
        completed_loads: u64_field(streaming, "completed_loads"),
        average_load_latency_frames: finite_f64(f64_field(
            streaming,
            "average_load_latency_frames",
        )),
        max_load_latency_frames: u64_field(streaming, "max_load_latency_frames"),
    })
}

#[inline]
fn u64_field(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or_default()
}

#[inline]
fn f64_field(value: &Value, name: &str) -> f64 {
    value.get(name).and_then(Value::as_f64).unwrap_or_default()
}

#[inline]
fn finite_f64(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(frame: u64, pressure: f64, misses: u64, latency: u64) -> JobRecord {
        JobRecord {
            id: format!("stream-{frame}"),
            name: "Asset Streaming Control Plane".to_owned(),
            category: "asset_streaming_control_plane".to_owned(),
            source: "newengine-assets".to_owned(),
            lane: "AssetIo".to_owned(),
            priority: "normal".to_owned(),
            dependency_group: "asset_streaming".to_owned(),
            frame_id: Some(frame),
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
            elapsed_ms: Some(0.2),
            budget_ms: 1.0,
            load: Some(0.2),
            progress: None,
            payload_bytes: None,
            output_bytes: None,
            error: None,
            metadata: json!({
                "schema": STREAMING_SAMPLE_SCHEMA,
                "frame_id": frame,
                "streaming": {
                    "budget_bytes": 1000,
                    "committed_bytes": (pressure * 1000.0) as u64,
                    "projected_resident_bytes": 900,
                    "pressure_ratio": pressure,
                    "pending_assets": frame,
                    "pending_demands": frame + 1,
                    "in_flight_assets": 2,
                    "in_flight_bytes": 100,
                    "over_budget_bytes": if pressure > 1.0 { 100 } else { 0 },
                    "deadline_misses": misses,
                    "total_deadline_misses": misses,
                    "provider_failures": if frame == 2 { 1 } else { 0 },
                    "admissions_dispatched": 1,
                    "evictions_dispatched": 0,
                    "completed_loads": 1,
                    "average_load_latency_frames": latency as f64,
                    "max_load_latency_frames": latency,
                }
            }),
        }
    }

    #[test]
    fn pressure_report_is_frame_ordered_and_tracks_peaks() {
        let jobs = VecDeque::from([
            record(2, 1.2, 3, 8),
            record(1, 0.7, 1, 4),
            record(3, 0.9, 4, 5),
        ]);
        let report = build_streaming_pressure(&jobs);
        assert_eq!(report["sample_count"], 3);
        assert_eq!(report["summary"]["peak_pressure_ratio"], 1.2);
        assert_eq!(report["summary"]["total_deadline_misses"], 4);
        assert_eq!(report["summary"]["peak_load_latency_frames"], 8);
        assert_eq!(report["summary"]["provider_failure_observations"], 1);
        assert_eq!(report["series"][0]["frame_id"], 1);
        assert_eq!(report["series"][2]["frame_id"], 3);
    }

    #[test]
    fn unrelated_profiler_samples_are_ignored() {
        let mut unrelated = record(1, 0.5, 0, 1);
        unrelated.metadata["schema"] = json!("other.schema");
        let report = build_streaming_pressure(&VecDeque::from([unrelated]));
        assert_eq!(report["sample_count"], 0);
        assert!(report["latest"].is_null());
    }
}

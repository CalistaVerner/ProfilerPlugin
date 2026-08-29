use std::collections::VecDeque;

use serde::Serialize;
use serde_json::{json, Value};

use crate::records::JobRecord;

const POPULATION_SAMPLE_SCHEMA: &str = "newengine.population.control_plane.profiler.v1";
const MAX_SERIES_SAMPLES: usize = 512;

#[derive(Debug, Clone, Serialize, PartialEq)]
struct PopulationPressureSample {
    frame_id: u64,
    elapsed_ms: f64,
    subjects: u64,
    managed_subjects: u64,
    protected_subjects: u64,
    spawn_requested: u64,
    spawn_admitted: u64,
    spawn_deferred: u64,
    retire_requested: u64,
    retire_admitted: u64,
    retire_deferred: u64,
    creation_debt: u64,
    destruction_debt: u64,
    starvation_promotions: u64,
    region_quota_clips: u64,
    teleport_rebuild: bool,
    teleport_rebuilds_total: u64,
    cpu_pressure: f64,
    model_memory_pressure: f64,
    density_scale: f64,
    model_pressure_rows: u64,
    peak_model_pressure: u64,
}

pub(crate) fn build_population_pressure(jobs: &VecDeque<JobRecord>) -> Value {
    let mut samples = jobs.iter().filter_map(sample_from_job).collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.frame_id);
    if samples.len() > MAX_SERIES_SAMPLES {
        samples.drain(..samples.len() - MAX_SERIES_SAMPLES);
    }

    let Some(latest) = samples.last().cloned() else {
        return json!({
            "schema": "newengine.profiler.population_pressure.v1",
            "sample_count": 0,
            "latest": null,
            "series": []
        });
    };

    let peak_creation_debt = samples
        .iter()
        .map(|sample| sample.creation_debt)
        .max()
        .unwrap_or_default();
    let peak_destruction_debt = samples
        .iter()
        .map(|sample| sample.destruction_debt)
        .max()
        .unwrap_or_default();
    let peak_cpu_pressure = samples
        .iter()
        .map(|sample| sample.cpu_pressure)
        .fold(0.0_f64, f64::max);
    let peak_model_memory_pressure = samples
        .iter()
        .map(|sample| sample.model_memory_pressure)
        .fold(0.0_f64, f64::max);
    let minimum_density_scale = samples
        .iter()
        .map(|sample| sample.density_scale)
        .filter(|value| value.is_finite())
        .fold(1.0_f64, f64::min);
    let total_spawn_deferred = samples
        .iter()
        .map(|sample| sample.spawn_deferred)
        .sum::<u64>();
    let total_retire_deferred = samples
        .iter()
        .map(|sample| sample.retire_deferred)
        .sum::<u64>();
    let total_starvation_promotions = samples
        .iter()
        .map(|sample| sample.starvation_promotions)
        .sum::<u64>();
    let peak_model_pressure = samples
        .iter()
        .map(|sample| sample.peak_model_pressure)
        .max()
        .unwrap_or_default();
    let teleport_rebuilds_total = samples
        .iter()
        .map(|sample| sample.teleport_rebuilds_total)
        .max()
        .unwrap_or_default();

    json!({
        "schema": "newengine.profiler.population_pressure.v1",
        "sample_count": samples.len(),
        "latest": latest,
        "summary": {
            "peak_creation_debt": peak_creation_debt,
            "peak_destruction_debt": peak_destruction_debt,
            "peak_cpu_pressure": peak_cpu_pressure,
            "peak_model_memory_pressure": peak_model_memory_pressure,
            "minimum_density_scale": minimum_density_scale,
            "total_spawn_deferred": total_spawn_deferred,
            "total_retire_deferred": total_retire_deferred,
            "total_starvation_promotions": total_starvation_promotions,
            "peak_model_pressure": peak_model_pressure,
            "teleport_rebuilds_total": teleport_rebuilds_total,
        },
        "series": samples,
    })
}

fn sample_from_job(job: &JobRecord) -> Option<PopulationPressureSample> {
    if job.metadata.get("schema").and_then(Value::as_str) != Some(POPULATION_SAMPLE_SCHEMA) {
        return None;
    }
    let population = job.metadata.get("population")?;
    let frame_id = job
        .frame_id
        .or_else(|| job.metadata.get("frame_id").and_then(Value::as_u64))?;
    Some(PopulationPressureSample {
        frame_id,
        elapsed_ms: finite_f64(job.elapsed_ms.unwrap_or_default()),
        subjects: u64_field(population, "subjects"),
        managed_subjects: u64_field(population, "managed_subjects"),
        protected_subjects: u64_field(population, "protected_subjects"),
        spawn_requested: u64_field(population, "spawn_requested"),
        spawn_admitted: u64_field(population, "spawn_admitted"),
        spawn_deferred: u64_field(population, "spawn_deferred"),
        retire_requested: u64_field(population, "retire_requested"),
        retire_admitted: u64_field(population, "retire_admitted"),
        retire_deferred: u64_field(population, "retire_deferred"),
        creation_debt: u64_field(population, "creation_debt"),
        destruction_debt: u64_field(population, "destruction_debt"),
        starvation_promotions: u64_field(population, "starvation_promotions"),
        region_quota_clips: u64_field(population, "region_quota_clips"),
        teleport_rebuild: population
            .get("teleport_rebuild")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        teleport_rebuilds_total: u64_field(population, "teleport_rebuilds_total"),
        cpu_pressure: finite_f64(f64_field(population, "cpu_pressure")),
        model_memory_pressure: finite_f64(f64_field(population, "model_memory_pressure")),
        density_scale: finite_f64(f64_field(population, "density_scale")),
        model_pressure_rows: u64_field(population, "model_pressure_rows"),
        peak_model_pressure: u64_field(population, "peak_model_pressure"),
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

    fn record(frame: u64, debt: u64, density: f64, teleport: bool) -> JobRecord {
        JobRecord {
            id: format!("population-{frame}"),
            name: "Population Control Plane".to_owned(),
            category: "population_control_plane".to_owned(),
            source: "newengine-population-runtime".to_owned(),
            lane: "Simulation".to_owned(),
            priority: "normal".to_owned(),
            dependency_group: "population".to_owned(),
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
            elapsed_ms: Some(0.1),
            budget_ms: 1.0,
            load: Some(0.1),
            progress: None,
            payload_bytes: None,
            output_bytes: None,
            error: None,
            metadata: json!({
                "schema": POPULATION_SAMPLE_SCHEMA,
                "population": {
                    "subjects": 100,
                    "managed_subjects": 90,
                    "protected_subjects": 10,
                    "spawn_requested": 8,
                    "spawn_admitted": 4,
                    "spawn_deferred": 4,
                    "retire_requested": 3,
                    "retire_admitted": 2,
                    "retire_deferred": 1,
                    "creation_debt": debt,
                    "destruction_debt": debt / 2,
                    "starvation_promotions": frame,
                    "region_quota_clips": 1,
                    "teleport_rebuild": teleport,
                    "teleport_rebuilds_total": if teleport { 1 } else { 0 },
                    "cpu_pressure": 0.5,
                    "model_memory_pressure": 0.7,
                    "density_scale": density,
                    "model_pressure_rows": 4,
                    "peak_model_pressure": 920,
                }
            }),
        }
    }

    #[test]
    fn population_report_tracks_debt_density_and_teleports() {
        let jobs = VecDeque::from([
            record(2, 9, 0.5, true),
            record(1, 3, 0.8, false),
            record(3, 4, 0.7, false),
        ]);
        let report = build_population_pressure(&jobs);
        assert_eq!(report["sample_count"], 3);
        assert_eq!(report["summary"]["peak_creation_debt"], 9);
        assert_eq!(report["summary"]["minimum_density_scale"], 0.5);
        assert_eq!(report["summary"]["teleport_rebuilds_total"], 1);
        assert_eq!(report["series"][0]["frame_id"], 1);
    }

    #[test]
    fn unrelated_samples_are_ignored() {
        let mut unrelated = record(1, 2, 1.0, false);
        unrelated.metadata["schema"] = json!("other.schema");
        let report = build_population_pressure(&VecDeque::from([unrelated]));
        assert_eq!(report["sample_count"], 0);
    }
}

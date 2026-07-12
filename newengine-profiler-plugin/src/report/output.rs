use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::archive::{write_stored_zip, ZipFileEntry};
use crate::records::ReportPaths;
use crate::runtime::ProfilerRuntime;
use crate::util::{path_to_string, unix_ms, utc_stamp_from_unix_ms, write_file};

use super::csv::*;

const CSV_LIMIT: usize = 100_000;

struct CsvArtifact {
    kind: &'static str,
    latest_name: String,
    timestamped_name: String,
    bytes: Vec<u8>,
}

struct ArchiveManifestInput<'a> {
    report: &'a Value,
    paths: &'a ReportPaths,
    created_utc: &'a str,
    created_unix_ms: u128,
    json_entry_name: Option<&'a str>,
    markdown_entry_name: Option<&'a str>,
    csv_entry_names: &'a BTreeMap<String, String>,
}

impl ProfilerRuntime {
    pub(super) fn write_report_files(
        &self,
        report: &Value,
        markdown: &str,
    ) -> Result<(ReportPaths, usize), String> {
        let dir = PathBuf::from(&self.cfg.report.directory);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("create report directory '{}' failed: {e}", dir.display()))?;
        let created_unix_ms = unix_ms();
        let created_utc = utc_stamp_from_unix_ms(created_unix_ms);
        let archive_prefix = safe_archive_prefix(&self.cfg.report.archive_prefix);
        let archive_name = format!("{archive_prefix}_{created_utc}.zip");
        let archive_path = dir.join(&archive_name);
        let json_entry_name = format!("profiler_report_{created_utc}.json");
        let markdown_entry_name = format!("profiler_report_{created_utc}.md");
        let manifest_entry_name = "manifest.json".to_owned();

        let mut paths = ReportPaths {
            archive: None,
            archive_created_unix_ms: None,
            archive_created_utc: None,
            archive_manifest: None,
            json_latest: None,
            json_timestamped: None,
            markdown_latest: None,
            markdown_timestamped: None,
            csv_latest: None,
            csv_timestamped: None,
        };

        let json_bytes = if self.cfg.report.write_json {
            let bytes = serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?;
            let latest = dir.join(&self.cfg.report.latest_json);
            write_file(&latest, &bytes)?;
            paths.json_latest = Some(path_to_string(&latest));
            Some(bytes)
        } else {
            None
        };

        let markdown_bytes = if self.cfg.report.write_markdown {
            let latest = dir.join(&self.cfg.report.latest_markdown);
            write_file(&latest, markdown.as_bytes())?;
            paths.markdown_latest = Some(path_to_string(&latest));
            Some(markdown.as_bytes().to_vec())
        } else {
            None
        };

        let csv_artifacts = if self.cfg.report.write_csv {
            self.build_csv_artifacts(report, &created_utc)?
        } else {
            Vec::new()
        };
        let csv_total_bytes = csv_artifacts.iter().map(|a| a.bytes.len()).sum::<usize>();

        if !csv_artifacts.is_empty() {
            let mut latest = BTreeMap::new();
            for artifact in &csv_artifacts {
                let latest_path = dir.join(&artifact.latest_name);
                write_file(&latest_path, &artifact.bytes)?;
                latest.insert(artifact.kind.to_owned(), path_to_string(&latest_path));
            }
            paths.csv_latest = Some(latest);
        }

        if self.cfg.report.write_archive {
            let archive_path_string = path_to_string(&archive_path);
            if json_bytes.is_some() {
                paths.json_timestamped = Some(format!("{archive_path_string}#{json_entry_name}"));
            }
            if markdown_bytes.is_some() {
                paths.markdown_timestamped =
                    Some(format!("{archive_path_string}#{markdown_entry_name}"));
            }
            if !csv_artifacts.is_empty() {
                let mut csv_timestamped = BTreeMap::new();
                for artifact in &csv_artifacts {
                    csv_timestamped.insert(
                        artifact.kind.to_owned(),
                        format!("{archive_path_string}#{}", artifact.timestamped_name),
                    );
                }
                paths.csv_timestamped = Some(csv_timestamped);
            }
            paths.archive = Some(archive_path_string.clone());
            paths.archive_created_unix_ms = Some(created_unix_ms);
            paths.archive_created_utc = Some(created_utc.clone());
            paths.archive_manifest = Some(format!("{archive_path_string}#{manifest_entry_name}"));

            let csv_entry_names = csv_artifacts
                .iter()
                .map(|a| (a.kind.to_owned(), a.timestamped_name.clone()))
                .collect::<BTreeMap<_, _>>();
            let manifest = self.build_report_archive_manifest(ArchiveManifestInput {
                report,
                paths: &paths,
                created_utc: &created_utc,
                created_unix_ms,
                json_entry_name: json_bytes.as_ref().map(|_| json_entry_name.as_str()),
                markdown_entry_name: markdown_bytes
                    .as_ref()
                    .map(|_| markdown_entry_name.as_str()),
                csv_entry_names: &csv_entry_names,
            });
            let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;

            let mut entries = Vec::new();
            entries.push(ZipFileEntry {
                name: manifest_entry_name,
                bytes: &manifest_bytes,
            });
            if let Some(bytes) = json_bytes.as_ref() {
                entries.push(ZipFileEntry {
                    name: json_entry_name.clone(),
                    bytes,
                });
                if self.cfg.report.include_latest_in_archive {
                    entries.push(ZipFileEntry {
                        name: self.cfg.report.latest_json.clone(),
                        bytes,
                    });
                }
            }
            if let Some(bytes) = markdown_bytes.as_ref() {
                entries.push(ZipFileEntry {
                    name: markdown_entry_name.clone(),
                    bytes,
                });
                if self.cfg.report.include_latest_in_archive {
                    entries.push(ZipFileEntry {
                        name: self.cfg.report.latest_markdown.clone(),
                        bytes,
                    });
                }
            }
            for artifact in &csv_artifacts {
                entries.push(ZipFileEntry {
                    name: artifact.timestamped_name.clone(),
                    bytes: &artifact.bytes,
                });
                if self.cfg.report.include_latest_in_archive {
                    entries.push(ZipFileEntry {
                        name: artifact.latest_name.clone(),
                        bytes: &artifact.bytes,
                    });
                }
            }
            write_stored_zip(&archive_path, created_unix_ms, &entries)?;
        } else {
            if let Some(bytes) = json_bytes.as_ref() {
                let stamped = dir.join(&json_entry_name);
                write_file(&stamped, bytes)?;
                paths.json_timestamped = Some(path_to_string(&stamped));
            }
            if let Some(bytes) = markdown_bytes.as_ref() {
                let stamped = dir.join(&markdown_entry_name);
                write_file(&stamped, bytes)?;
                paths.markdown_timestamped = Some(path_to_string(&stamped));
            }
            if !csv_artifacts.is_empty() {
                let mut timestamped = BTreeMap::new();
                for artifact in &csv_artifacts {
                    let stamped = dir.join(&artifact.timestamped_name);
                    write_file(&stamped, &artifact.bytes)?;
                    timestamped.insert(artifact.kind.to_owned(), path_to_string(&stamped));
                }
                paths.csv_timestamped = Some(timestamped);
            }
        }

        Ok((paths, csv_total_bytes))
    }

    fn build_report_archive_manifest(&self, input: ArchiveManifestInput<'_>) -> Value {
        json!({
            "schema": "newengine.profiler.report_archive.manifest.v2",
            "created_utc": input.created_utc,
            "created_unix_ms": input.created_unix_ms,
            "reason": input.report.get("reason").cloned().unwrap_or(Value::Null),
            "archive": input.paths.archive.clone(),
            "latest": {
                "json": input.paths.json_latest.clone(),
                "markdown": input.paths.markdown_latest.clone(),
                "csv": input.paths.csv_latest.clone(),
            },
            "entries": {
                "json": input.json_entry_name,
                "markdown": input.markdown_entry_name,
                "csv": input.csv_entry_names,
                "manifest": "manifest.json",
            },
            "policy": {
                "timestamped_files_are_archive_members": self.cfg.report.write_archive,
                "latest_files_are_written_for_compatibility": true,
                "latest_files_are_duplicated_in_archive": self.cfg.report.include_latest_in_archive,
                "csv_files_are_machine_readable_source_for_external_charts": self.cfg.report.write_csv,
            },
        })
    }

    fn build_csv_artifacts(
        &self,
        report: &Value,
        created_utc: &str,
    ) -> Result<Vec<CsvArtifact>, String> {
        let specs = [
            (
                "jobs",
                self.cfg.report.latest_jobs_csv.clone(),
                format!("profiler_jobs_{created_utc}.csv"),
                csv_completed_jobs(report),
            ),
            (
                "categories",
                self.cfg.report.latest_categories_csv.clone(),
                format!("profiler_categories_{created_utc}.csv"),
                csv_category_summary(report),
            ),
            (
                "sources",
                self.cfg.report.latest_sources_csv.clone(),
                format!("profiler_sources_{created_utc}.csv"),
                csv_source_summary(report),
            ),
            (
                "top_offenders",
                self.cfg.report.latest_offenders_csv.clone(),
                format!("profiler_top_offenders_{created_utc}.csv"),
                csv_top_offenders(report),
            ),
            (
                "active_jobs",
                self.cfg.report.latest_active_csv.clone(),
                format!("profiler_active_jobs_{created_utc}.csv"),
                csv_active_jobs(report),
            ),
            (
                "diagnostics",
                self.cfg.report.latest_diagnostics_csv.clone(),
                format!("profiler_diagnostics_{created_utc}.csv"),
                csv_diagnostics(report),
            ),
            (
                "timeline",
                self.cfg.report.latest_timeline_csv.clone(),
                format!("profiler_timeline_{created_utc}.csv"),
                csv_timeline(report),
            ),
            (
                "methods",
                self.cfg.report.latest_methods_csv.clone(),
                format!("profiler_methods_{created_utc}.csv"),
                csv_methods(report),
            ),
            (
                "budget_violations",
                self.cfg.report.latest_budget_violations_csv.clone(),
                format!("profiler_budget_violations_{created_utc}.csv"),
                csv_budget_violations(report),
            ),
            (
                "lanes",
                self.cfg.report.latest_lanes_csv.clone(),
                format!("profiler_lanes_{created_utc}.csv"),
                csv_lanes(report),
            ),
            (
                "profiler_first",
                self.cfg.report.latest_profiler_first_csv.clone(),
                format!("profiler_first_{created_utc}.csv"),
                csv_profiler_first(report),
            ),
            (
                "frame_budget",
                self.cfg.report.latest_frame_budget_csv.clone(),
                format!("profiler_frame_budget_{created_utc}.csv"),
                csv_frame_budget(report),
            ),
        ];

        let mut artifacts = Vec::with_capacity(specs.len());
        for (kind, latest_name, timestamped_name, content) in specs {
            let bytes = content.into_bytes();
            if bytes.len() > CSV_LIMIT * 1024 * 1024 {
                return Err(format!(
                    "profiler CSV artifact '{kind}' is unexpectedly large: {} bytes",
                    bytes.len()
                ));
            }
            artifacts.push(CsvArtifact {
                kind,
                latest_name,
                timestamped_name,
                bytes,
            });
        }
        Ok(artifacts)
    }
}

fn safe_archive_prefix(value: &str) -> String {
    let sanitized: String = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches('.').trim_matches('_').to_owned();
    if sanitized.is_empty() {
        "profiler_report".to_owned()
    } else {
        sanitized
    }
}

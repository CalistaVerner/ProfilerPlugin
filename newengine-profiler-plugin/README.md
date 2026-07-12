# newengine-profiler-plugin

<!-- NORTHSTAR-DIR-README:BEGIN -->

## Directory purpose

**Path:** `PluginsSrc/ProfilerPlugin/newengine-profiler-plugin`

**Role:** Source code for this crate/module/provider.

**Local contents:** 2 direct subdirectories, 2 direct files.

**Direct file examples:** `build.rs`, `Cargo.toml`

## Working rules

- Do not put transient build output in this directory unless the directory is explicitly a runtime output/cache location.
- Keep runtime assets and editable source assets separate: source assets are packed into runtime formats through explicit tools/manifests.
- Do not introduce hidden provider/backend coupling here; use declared descriptors, gateways, DTOs, and explicit maintenance scripts.

<!-- NORTHSTAR-DIR-README:END -->

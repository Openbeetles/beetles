# Architecture Overview

**English** | [中文](../zh-cn/architecture.md) | [Doc index](../README.md)

This page is a quick architecture guide for developers extending Beetle.

This is a developer document, not a first-time user guide.

It covers:

- the main modules
- the high-level data flow
- where to extend channels, tools, and LLM backends

## Core Idea

At a high level, Beetle works like this:

1. channels push messages into the inbound queue
2. the agent builds context and calls the LLM
3. tools and memory are used during the loop
4. replies are pushed into the outbound queue
5. dispatch sends them through the right channel

## Main Modules

| Module | What it does |
|--------|--------------|
| `config` | Load and validate config from env, NVS, and SPIFFS |
| `error` | Shared error type and stage-based error reporting |
| `bus` | Inbound and outbound queues |
| `orchestrator` | Runtime resource gating, pressure tracking, TLS-fragmentation awareness, and health state |
| `memory` | Session state, archive evidence, the shared factual plane, self continuity layers, and prompt context |
| `platform` | Platform abstraction and platform-specific implementations |
| `llm` | LLM clients and fallback routing |
| `tools` | Tool definitions and runtime registry |
| `agent` | ReAct loop, context build, tool-use loop, session writes |
| `channels` | Channel ingress, egress, and dispatch plumbing |
| `display` | SPI display config and dashboard rendering |
| `metrics` | Runtime counters, error aggregation, and snapshots |

Optional feature areas include `cli` and `ota`.

## Data Flow

```text
channel -> inbound queue -> agent -> tools / memory / llm -> outbound queue -> dispatch -> channel sender
```

More concretely:

- inbound messages come from chat channels or scheduled tasks
- the agent pulls one message, builds context, and runs the LLM/tool loop; shared facts, archive evidence, and private continuity layers are assembled separately
- exact factual retrieval prefers slot lookup / `factual_memory`, while archive retrieval stays evidence-only
- session and memory state are updated, then post-reply maintenance and `self_runtime` may trigger boundary flush work
- background-only LLM jobs such as post-reply maintenance, long-term memory refresh, and `self_runtime` are routed through `agent/loop/background_jobs.rs` so the foreground turn loop and system jobs stay on one execution path
- the final reply goes to outbound dispatch

On ESP, the runtime resource path also treats the largest internal free block as a first-class signal:

- TLS admission and runtime pressure both consult the same `heap_largest_block_internal` snapshot
- `/api/resource` exposes the derived `tls_fragmentation_risk` so the operator surface and heartbeat share the same interpretation
- ESP startup now emits `[orchestrator] startup memory checkpoint ...` lines around config, WiFi, audio, control-plane, sender, and agent bring-up so operators can locate the exact stage where the internal largest free block collapses
- control-plane diagnostics such as `GET /api/channel_connectivity` must degrade to stale snapshots instead of forcing fresh outbound TLS probes when WiFi is still settling or fragmentation risk is already elevated
- external WSS reconnect loops must also pause under critical pressure instead of retrying token / gateway fetches into a known TLS-admission failure window

## Memory Mainline

Beetle no longer treats memory as one flat text surface. The mainline is layered:

- `archive plane`: transcript, daily, and turn-log evidence used for retrieval and reconciliation
- `shared factual plane`: canonical shared records for stable facts, constraints, and slot-shaped project/task/profile state
- `private continuity layers`: `self_model`, `inner_life`, `self_continuity`, `private_docs`, and `private_garden`

These layers are intentionally separated:

- archive hits are evidence, not final truth by themselves
- the shared factual plane is where reusable canonical conclusions live
- private layers serve continuity/personality and are not exposed by default

Continuity migration and reboot handoff now sit on this same path:

- `continuity_snapshot` supports bootstrap/full-restore export and import
- restart paths try to flush a recent continuity bundle before reboot so handoff/reboot does not sever continuity

## Extension Points

### Add a new channel

- implement the channel-specific ingress/egress logic
- register its sink in the dispatch setup
- feed inbound messages into the bus

### Add a new tool

- implement the `Tool` trait
- define `name`, `description`, `schema`, and execution logic
- register it in `build_default_registry`

### Add a new LLM backend

- implement the `LlmClient` trait
- wire it into the client build path in `llm`

## One Important Boundary

Most of the codebase talks to the platform through abstractions.

The main intentional exception is ESP-specific WSS transport under `channels/wss_gateway/esp_conn.rs`, which directly depends on ESP-IDF-side support.

There are two additional ESP persistence rules worth keeping explicit:

- State-file and SPIFFS access must go through the `platform::spiffs` / `StateFs` boundary. Do not add ad-hoc `std::fs` reads or writes against `/spiffs` or `state_mount_path()` from business modules.
- Display refresh, presence polling, and other hot loops must not repeatedly scan SPIFFS. Persistent status that needs to appear in those loops should come from caches, snapshots, or explicit recovery/update paths instead of periodic flash reads.
- Persistent artifacts such as the reboot `runtime_bundle`, which only change on recovery/restart boundaries, must use invalidation-driven caching on ESP. Do not leave bundle file reads in display/presence polling paths.
- On ESP startup, `soul_kernel recovery` must complete before WiFi bring-up starts. Do not overlap reboot-recovery SPIFFS reads with the asynchronous WiFi startup window.
- ESP startup recovery must not run inline on `main task`. Run `soul_kernel recovery` on a dedicated startup-recovery execution plane, wait for it to finish, and only then continue into config load and WiFi bring-up so continuity import / serde / SPIFFS work does not consume `CONFIG_ESP_MAIN_TASK_STACK_SIZE`.
- On Linux, the config HTTP plane and the supervisor-owned control plane must share the same `HandlerContext` assembly. Differences in exposed routes belong in `ControlPlaneRouteContract`, not in duplicated handler wiring.

## Related Docs

- [tools.md](tools.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)

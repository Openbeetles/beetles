# Architecture Overview

**English** | [中文](../zh-cn/architecture.md) | [Doc index](../README.md)

This page is a quick architecture guide for developers extending Beetle.

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
| `orchestrator` | Runtime resource gating, pressure tracking, and health state |
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
- the final reply goes to outbound dispatch

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

## Related Docs

- [tools.md](tools.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)

# Architecture Overview

**English** | [中文](../zh-cn/architecture.md) | [Doc index](../README.md)

This page is for developers.
Read it first if you want to extend Beetle or get your bearings in the codebase.

## Main Layers

| Layer | Main job |
|-------|----------|
| `config` / `platform` | load settings and connect Beetle to system and hardware capabilities |
| `channels` | receive and send messages |
| `agent` | understand input, decide what to do, and build replies |
| `tools` / `memory` | call outside capabilities and store important information |
| `runtime` | manage runtime state, resources, and health |
| `display` / HTTP API | expose the screen, setup page, and status surface |

## How a message flows

1. a message enters through a channel
2. the agent reads the current context and saved information
3. it uses tools or services when needed
4. it sends the reply back through the right channel
5. it saves important results

## Where you usually extend Beetle

### Add a new channel

- implement channel send and receive logic in `channels`
- feed inbound messages into the bus
- register the sender in dispatch

### Add a new tool

- implement the `Tool` trait
- define its description, schema, and execution logic
- register it in `build_default_registry`

### Add a new model backend

- implement the `LlmClient` trait
- wire it into the existing client build path

### Add a new platform

- implement the `Platform` trait
- inject it from the assembly entrypoint

## Boundaries worth keeping

- keep hardware-specific code inside `platform`
- keep business logic independent from hardware implementations
- use the shared `beetle::Error` type
- pass config explicitly instead of relying on mutable global state
- update docs together with code changes

## Related Docs

- [configuration.md](configuration.md)
- [tools.md](tools.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)

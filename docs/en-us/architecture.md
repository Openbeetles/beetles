# Architecture

**English** | [中文](../zh-cn/architecture.md) | [Doc index](../README.md)

Beetle OS is organized into the layers below.
If you only need setup or deployment, go back to [getting-started-esp.md](getting-started-esp.md), [getting-started-linux.md](getting-started-linux.md), or [build-script.md](build-script.md).

## Main Layers

| Layer | Main job |
|-------|----------|
| `config` / `platform` | load settings and connect Beetle OS to system and hardware capabilities |
| `channels` | receive and send messages |
| `agent` | understand input, decide what to do, and build replies |
| `tools` / `memory` | call outside capabilities and store important information |
| `runtime` | manage runtime state, resources, and health |
| `display` / HTTP API | expose the screen, setup page, and status surface |

## How A Message Flows

1. a message enters through a channel
2. the agent reads current context and saved information
3. it uses tools or services when needed
4. it sends the reply back through the right channel
5. it saves important results

## Common Extension Points

### Add A New Channel

- implement channel send and receive logic in `channels`
- feed inbound messages into the bus
- register the sender in dispatch

### Add A New Tool

- implement the `Tool` trait
- define its description, schema, and execution logic
- register it in `build_default_registry`

### Add A New Model Backend

- implement the `LlmClient` trait
- wire it into the existing client build path

### Add A New Platform

- implement the `Platform` trait
- inject it from the assembly entrypoint

## Boundaries Worth Keeping

- keep hardware-specific code inside `platform`
- keep business logic independent from hardware implementations
- use the shared `beetle::Error` type
- pass config explicitly instead of relying on mutable global state
- update docs together with code changes

## Related Docs

- [capabilities.md](capabilities.md)
- [config-api.md](config-api.md)
- [hardware.md](hardware.md)

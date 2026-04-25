# Get Started on Linux

[中文](../zh-cn/getting-started-linux.md) | **English** | [Doc index](../README.md)

This page gets Beetle OS from a Linux build to a first successful chat reply.

## Before You Begin

- one Linux target host
- SSH access if build and deployment happen from another machine
- one model provider account or local model endpoint
- one chat channel you plan to test first

## 1. Build the Linux Artifact

Common starting commands:

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
TARGET=linux ./build.sh --package-linux
```

If you already have a built artifact, you can skip ahead to deployment.
For the simplest release path, prefer `TARGET=linux ./build.sh --package-linux`; `BUILD_METHOD=auto` will try Docker first on macOS, then a saved remote Linux host, and only then a local cross-build.

## 2. Deploy Beetle OS

The standard deployment entry is:

```bash
./build.sh --deploy-linux
```

If you need the full Linux release layout, restart behavior, or rollback flow, read [linux-release-rollback.md](linux-release-rollback.md).

## 3. Open the Setup Page

After deployment, open the Configure UI from the hosted page, desktop shell, or a locally served build, then enter the deployed host address as the **Device URL**.
The service root returns API inventory JSON and should not be treated as an embedded setup page.

The normal next step is the browser-based setup flow, where you save:

1. pairing code
2. network or proxy settings if needed
3. one LLM source
4. one chat channel

## 4. Send a First Message

After the first model and first chat channel are saved:

1. open the configured chat channel
2. send a short test message through that channel
3. confirm it replies once without manual recovery

## 5. Check the Running Service

For Linux installs, it is worth checking the runtime state once:

```bash
beetle release status
```

If you need to manage the runtime directly later, the common commands are:

- `beetle restart`
- `beetle stop`
- `beetle release rollback`

## If You Do Not Get a Reply

- check that the Linux artifact was deployed to the active release path
- check that model settings are valid
- check that the selected chat channel is configured correctly
- check release state and runtime status

## Next Steps

- For Linux deploy, restart, stop, and rollback: [linux-release-rollback.md](linux-release-rollback.md)
- For the browser setup areas: [configuration.md](configuration.md)
- To understand Beetle OS capability surface: [capabilities.md](capabilities.md)
- To build your own integration: [config-api.md](config-api.md)

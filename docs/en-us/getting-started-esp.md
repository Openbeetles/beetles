# Get Started on ESP32

[中文](../zh-cn/getting-started-esp.md) | **English** | [Doc index](../README.md)

This page gets Beetle OS from an ESP32 build to a first successful chat reply.

## Before You Begin

- one supported board
- one USB cable for flashing
- one network the device can join after setup
- one model provider account or local model endpoint
- one chat channel you plan to test first

## 1. Build and Flash Beetle OS

The usual starting command is:

```bash
./build.sh --flash
```

Common board examples:

```bash
BOARD=esp32-s3-16mb ./build.sh --flash
BOARD=esp32-p4-nano-16mb ./build.sh --flash
./build.sh flash-all
```

If you need more flashing and target-selection detail, read [build-script.md](build-script.md).

## 2. Open the Setup Page

On first use, the device usually exposes a hotspot named **Beetle**.

1. connect to that hotspot
2. open the Configure UI from the hosted page, desktop shell, or a locally served build
3. enter `http://192.168.4.1` as the **Device URL**

If the device is already on your local network, enter its current local IP as the **Device URL** instead.
The device root itself returns API inventory JSON and is not a promised embedded UI or redirect.

## 3. Finish the Minimum Setup

For a first working loop, configure these in order:

1. pairing code
2. network
3. one LLM source
4. one chat channel

Leave hardware, display, audio, and office accounts for later unless you need them right away.

## 4. Send a First Message

After saving the first model and first chat channel:

1. open the configured chat channel
2. send a short test message through that channel
3. confirm it replies once without manual intervention

That is the first success condition.

## If You Do Not Get a Reply

- check that network settings were saved successfully
- check that the model source is valid and reachable
- check that the chat channel configuration is complete
- reopen the setup page and make sure the pairing code is correct when saving

## Next Steps

- To understand the setup areas in more detail: [configuration.md](configuration.md)
- To choose and configure a model provider: [llm-providers.md](llm-providers.md)
- To see what Beetle OS can do after setup: [capabilities.md](capabilities.md)
- To connect hardware or a display: [hardware.md](hardware.md), [hardware-device-config.md](hardware-device-config.md), [display.md](display.md)

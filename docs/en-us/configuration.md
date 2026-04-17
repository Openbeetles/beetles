# Configuration Guide

[中文](../zh-cn/configuration.md) | **English** | [Doc index](../README.md)

This page is only about getting Beetle working for the first time.

## Short Path

For first-time setup, do these four things first:

1. find the device address
2. set the pairing code
3. connect the device to your network
4. choose one LLM source and one chat channel

## Open the setup page first

There are two common ways to reach it:

- On first use, connect to the device hotspot **Beetle** and open `http://192.168.4.1`
- If the device is already on your local network, open its local IP

The repo also includes `configure-ui`, which can connect to the device.

## What the pairing code is for

The pairing code protects important actions such as:

- saving settings
- restarting the device
- resetting settings
- starting an online update

Set it the first time you open the page, and keep it for later.

## What you will usually configure

| Area | What it is for |
|------|----------------|
| Network | connect the device to your network |
| LLM | let Beetle understand and reply |
| Chat channels | use Beetle from your chat app |
| Work accounts | let Beetle handle mail, calendar, documents, and contacts |
| Hardware | control devices and read sensors |
| Display | show status on a screen |
| Audio | enable voice input and output |

## Suggested order

If your goal is simply to start using Beetle, this order is enough:

1. network
2. LLM
3. chat channel

You can add these later if needed:

- work accounts
- hardware
- display
- audio

## Common Problems

- Cannot open the setup page: make sure you are on the device hotspot or the same local network
- Cannot save settings: the pairing code is often wrong, or the page is stale, so reopen it and try again
- The page opens but Beetle does not reply: check that both the LLM and chat channel are set up
- Mail, calendar, or documents are missing: make sure the related account has been connected
- Hardware does not respond: make sure the device is configured and the wiring matches the setup

If you want to build your own frontend or script, read [config-api.md](config-api.md).

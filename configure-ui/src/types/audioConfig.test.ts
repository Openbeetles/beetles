import assert from "node:assert/strict";
import test from "node:test";
import {
  defaultAudioConfig,
  normalizeAudioConfigForSave,
  normalizeAudioConfigFromDevice,
} from "./audioConfig.ts";

test("default realtime instructions use the product-facing Beetle OS name", () => {
  const config = defaultAudioConfig();

  assert.match(config.realtime.instructions, /Beetle OS/);
  assert.doesNotMatch(config.realtime.instructions, /Beetles OS/);
});

test("normalizeAudioConfigFromDevice defaults missing topology to discrete_i2s", () => {
  const config = normalizeAudioConfigFromDevice({
    enabled: true,
    codec: {
      input_codec: "es7210",
      output_codec: "es8311",
      input_addr: 0x40,
      output_addr: 0x18,
      pa_pin: 46,
      input_reference: true,
    },
  });

  assert.equal(config.topology, "discrete_i2s");
  assert.equal(config.codec.input_codec, "es7210");
  assert.equal(config.codec.output_codec, "es8311");
  assert.equal(config.codec.input_reference, true);
});

test("normalizeAudioConfigForSave trims codec strings and preserves topology", () => {
  const config = normalizeAudioConfigForSave({
    ...defaultAudioConfig(),
    topology: "i2s_codec",
    codec: {
      input_codec: " es7210 ",
      output_codec: " es8311 ",
      input_addr: 0x40,
      output_addr: 0x18,
      pa_pin: 5,
      input_reference: true,
    },
  });

  assert.equal(config.topology, "i2s_codec");
  assert.equal(config.codec.input_codec, "es7210");
  assert.equal(config.codec.output_codec, "es8311");
  assert.equal(config.codec.pa_pin, 5);
  assert.equal(config.codec.input_reference, true);
});

test("default codec topology keeps mic and speaker sample rates aligned", () => {
  const config = defaultAudioConfig();

  assert.equal(config.topology, "discrete_i2s");
  assert.equal(config.microphone.sample_rate, config.speaker.sample_rate);
});

#!/usr/bin/env bash
set -euo pipefail

source_file="src/tools/analyze_image.rs"
llm_mod="src/llm/mod.rs"

require() {
  local pattern="$1"
  local message="$2"
  if ! rg -n "$pattern" "$source_file" "$llm_mod" >/dev/null; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

reject() {
  local pattern="$1"
  local message="$2"
  if rg -n "$pattern" "$source_file" >/dev/null; then
    echo "FAIL: $message" >&2
    rg -n "$pattern" "$source_file" >&2
    exit 1
  fi
}

require 'pub\(crate\) use request_body::LlmRequestBody' \
  "vision body writer must reuse the shared LlmRequestBody/ByteBuffer allocation path"
require 'build_openai_request_body|build_anthropic_request_body' \
  "analyze_image must have direct request body writers"
require 'push_base64_standard' \
  "local camera frame bytes must be base64-written into the request body"
require 'local_camera_openai_request_body_uses_external_preferred_buffer' \
  "OpenAI local camera body writer must have an external-preferred regression test"
require 'local_camera_anthropic_request_body_uses_external_preferred_buffer' \
  "Anthropic local camera body writer must have an external-preferred regression test"

reject 'serde_json::to_vec\(&body\)' \
  "analyze_image must not materialize serde_json::Value into a heap Vec request body"
reject 'json!\s*\(' \
  "analyze_image request builders must not use serde_json::json! intermediate Values"
reject 'format!\("data:\{media_type\};base64,\{data_base64\}"\)' \
  "OpenAI local frame path must not create a data URL String before writing JSON"
reject 'STANDARD\.encode\(frame\.bytes\.as_ref\(\)\)' \
  "local camera frame bytes must not be base64 encoded into a String first"

echo "analyze_image_body_contract_test: ok"

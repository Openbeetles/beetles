use crate::bus::{CanonicalMessageBody, CardBody, PlatformNativeBody, TextBody, TextFormat};
use crate::error::Result;
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolClarificationField,
    ToolClarificationOption, ToolContext, ToolEffectClass, ToolExecutionBlocker,
    ToolExecutionOutcome, ToolExecutionShape, ToolMetadata, ToolOutboundDeliveryKind,
    ToolOutboundIntent, ToolOutboundTarget, ToolRiskLevel, ToolRollbackKind,
};
use serde::Serialize;
use serde_json::{json, Value};

pub struct MessageTool;

#[derive(Serialize)]
struct MessageToolSummary<'a> {
    ok: bool,
    tool: &'static str,
    target: &'a str,
    delivery_kind: &'a str,
    channel: &'a str,
    chat_id: &'a str,
    sent_chars: usize,
    submitted_to_runtime: bool,
}

impl Tool for MessageTool {
    fn name(&self) -> &'static str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a user-visible message to an explicit channel/chat target through the runtime outbound pipeline. This tool is for cross-target delivery only; the current chat's reply surface is owned by the canonical finalizer."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"content":{"type":"string","description":"Text content or fallback text to send."},"body":{"description":"Canonical rich message body. When present it is authoritative."},"body_kind":{"type":"string","enum":["text","card","platform_native"],"description":"Shortcut body kind when not passing a full canonical body.","default":"text"},"text_format":{"type":"string","enum":["plain","markdown","html","rich_text"],"description":"Text formatting mode for text bodies.","default":"plain"},"payload_json":{"description":"Card or platform-native payload JSON for non-text bodies."},"platform_type":{"type":"string","description":"Platform-specific type when body_kind=platform_native."},"channel":{"type":"string","description":"Explicit target channel."},"chat_id":{"type":"string","description":"Explicit target chat id."},"delivery_kind":{"type":"string","enum":["supplemental","primary"],"description":"supplemental = visible update; primary = canonical reply for the explicit target.","default":"supplemental"}},"required":["channel","chat_id"],"anyOf":[{"required":["content"]},{"required":["body"]},{"required":["body_kind"]}]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        Ok(self.execute_outcome(args, ctx)?.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_message")?;
        if let Some(outcome) = missing_message_field_outcome(&obj) {
            return Ok(outcome);
        }
        let content = message_content(&obj);

        let delivery_kind = obj
            .get("delivery_kind")
            .and_then(Value::as_str)
            .unwrap_or("supplemental");
        let channel = obj
            .get("channel")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .expect("target blocker should have returned before channel extraction");
        let chat_id = obj
            .get("chat_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .expect("target blocker should have returned before chat extraction");
        if ctx.current_channel() == Some(channel) && ctx.current_chat_id() == Some(chat_id) {
            return Ok(ToolExecutionOutcome::text(message_ignored_payload(
                Some(channel),
                Some(chat_id),
                delivery_kind,
                "current-chat delivery is reserved for the canonical reply surface",
            )));
        }
        let channel = channel.to_string();
        let chat_id = chat_id.to_string();
        let body = build_message_body(&obj, content)?;
        let content_projection = body.text_projection();

        let primary = match delivery_kind {
            "supplemental" => false,
            "primary" => true,
            other => return invalid_delivery_kind_outcome(Some(&channel), Some(&chat_id), other),
        };
        let Some(capability) = ctx.channel_capability(&channel) else {
            return runtime_blocked_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "target channel capability is unavailable in this runtime context",
            );
        };
        if !capability.enabled {
            return runtime_blocked_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "target channel is not enabled in this runtime context",
            );
        }
        if primary && !capability.contract.supports_primary_reply {
            return unsupported_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "target channel does not support primary reply delivery",
            );
        }
        if !primary && !capability.contract.supports_supplemental_reply {
            return unsupported_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "target channel does not support supplemental reply delivery",
            );
        }
        if !capability.contract.supports_explicit_target {
            return unsupported_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "target channel does not support explicit outbound targets",
            );
        }
        if !capability
            .contract
            .supported_body_kinds
            .contains(&body.kind())
        {
            return unsupported_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "target channel does not support this outbound body kind",
            );
        }
        if let CanonicalMessageBody::Text(text) = &body {
            if !capability
                .contract
                .supported_text_formats
                .contains(&text.format)
            {
                return unsupported_message_outcome(
                    Some(&channel),
                    Some(&chat_id),
                    delivery_kind,
                    "target channel does not support this text format",
                );
            }
        }
        if !ctx.supports_explicit_outbound_message() {
            return runtime_blocked_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                "explicit outbound target is not allowed in this runtime context",
            );
        }
        if let Err(error) = ctx.claim_outbound_message_delivery(false, primary) {
            return runtime_blocked_message_outcome(
                Some(&channel),
                Some(&chat_id),
                delivery_kind,
                &format!(
                    "explicit outbound delivery is temporarily blocked in this runtime: {}",
                    error
                ),
            );
        }

        let summary = serialize_tool_output(
            "tool_message",
            &MessageToolSummary {
                ok: true,
                tool: "message",
                target: "explicit",
                delivery_kind: if primary { "primary" } else { "supplemental" },
                channel: channel.as_str(),
                chat_id: chat_id.as_str(),
                sent_chars: content_projection.chars().count(),
                submitted_to_runtime: true,
            },
        )?;

        Ok(
            ToolExecutionOutcome::text(summary).with_outbound_intent(ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit { channel, chat_id },
                delivery_kind: if primary {
                    ToolOutboundDeliveryKind::Primary
                } else {
                    ToolOutboundDeliveryKind::Supplemental
                },
                content: content_projection,
                body: Some(body),
            }),
        )
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_effect_class(ToolEffectClass::VisibleOutbound)
            .with_risk_level(ToolRiskLevel::High)
            .with_approval_mode(ToolApprovalMode::ExplicitIntent)
            .with_rollback_kind(ToolRollbackKind::Irreversible)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_message_governance")?;
        let delivery_kind = obj
            .get("delivery_kind")
            .and_then(Value::as_str)
            .unwrap_or("supplemental");
        Ok(self
            .metadata()
            .default_execution_shape("message_visible_delivery")
            .with_risk_level(if delivery_kind == "primary" {
                ToolRiskLevel::High
            } else {
                ToolRiskLevel::Medium
            })
            .with_approval_mode(ToolApprovalMode::ExplicitIntent)
            .with_approval_granted(true))
    }

    fn governance_examples(&self) -> &'static [&'static str] {
        &[
            r#"{"channel":"telegram","chat_id":"demo","delivery_kind":"supplemental","content":"hi"}"#,
            r#"{"channel":"telegram","chat_id":"demo","delivery_kind":"primary","content":"hi"}"#,
            r##"{"channel":"telegram","chat_id":"demo","delivery_kind":"supplemental","content":"# Status","text_format":"markdown"}"##,
            r#"{"channel":"feishu","chat_id":"demo","delivery_kind":"primary","body_kind":"card","content":"Order created","payload_json":{"header":{"title":"Order created"}}}"#,
        ]
    }
}

fn message_blocked_payload(
    channel: Option<&str>,
    chat_id: Option<&str>,
    delivery_kind: &str,
    warning: &str,
) -> String {
    json!({
        "ok": false,
        "tool": "message",
        "target": "explicit",
        "delivery_kind": delivery_kind,
        "channel": channel,
        "chat_id": chat_id,
        "sent_chars": 0,
        "submitted_to_runtime": false,
        "warning": warning,
    })
    .to_string()
}

fn message_ignored_payload(
    channel: Option<&str>,
    chat_id: Option<&str>,
    delivery_kind: &str,
    warning: &str,
) -> String {
    json!({
        "ok": true,
        "tool": "message",
        "target": "explicit",
        "delivery_kind": delivery_kind,
        "channel": channel,
        "chat_id": chat_id,
        "sent_chars": 0,
        "submitted_to_runtime": false,
        "warning": warning,
    })
    .to_string()
}

fn message_content(obj: &serde_json::Map<String, Value>) -> Option<&str> {
    obj.get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn message_body_kind_hint(obj: &serde_json::Map<String, Value>) -> Option<&str> {
    obj.get("body_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_text_format(value: Option<&str>) -> Result<TextFormat> {
    match value.unwrap_or("plain") {
        "plain" => Ok(TextFormat::Plain),
        "markdown" => Ok(TextFormat::Markdown),
        "html" => Ok(TextFormat::Html),
        "rich_text" => Ok(TextFormat::RichText),
        other => Err(crate::error::Error::config(
            "tool_message",
            format!("text_format must be one of plain, markdown, html, rich_text; got {other}"),
        )),
    }
}

fn build_message_body(
    obj: &serde_json::Map<String, Value>,
    content: Option<&str>,
) -> Result<CanonicalMessageBody> {
    if let Some(body_value) = obj.get("body") {
        return serde_json::from_value::<CanonicalMessageBody>(body_value.clone()).map_err(
            |error| {
                crate::error::Error::config(
                    "tool_message",
                    format!("body must match CanonicalMessageBody: {error}"),
                )
            },
        );
    }

    match message_body_kind_hint(obj).unwrap_or("text") {
        "text" => Ok(CanonicalMessageBody::Text(TextBody {
            text: content.unwrap_or_default().to_string(),
            format: parse_text_format(obj.get("text_format").and_then(Value::as_str))?,
        })),
        "card" => Ok(CanonicalMessageBody::Card(CardBody {
            format: crate::bus::CardFormat::Interactive,
            payload_json: obj
                .get("payload_json")
                .cloned()
                .unwrap_or_else(|| json!({})),
            fallback_text: content.unwrap_or_default().to_string(),
        })),
        "platform_native" => {
            let platform_type = obj
                .get("platform_type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    crate::error::Error::config(
                        "tool_message",
                        "platform_type is required when body_kind=platform_native",
                    )
                })?;
            Ok(CanonicalMessageBody::PlatformNative(PlatformNativeBody {
                platform_type: platform_type.to_string(),
                payload_json: obj
                    .get("payload_json")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                fallback_text: content.unwrap_or_default().to_string(),
            }))
        }
        other => Err(crate::error::Error::config(
            "tool_message",
            format!("body_kind must be one of text, card, platform_native; got {other}"),
        )),
    }
}

fn missing_message_field_outcome(
    obj: &serde_json::Map<String, Value>,
) -> Option<ToolExecutionOutcome> {
    let mut missing_fields = Vec::new();
    let mut clarification_fields = Vec::new();
    let requires_text_content = obj.get("body").is_none()
        && !matches!(
            message_body_kind_hint(obj),
            Some("card") | Some("platform_native")
        );
    if requires_text_content && message_content(obj).is_none() {
        missing_fields.push("content".to_string());
        clarification_fields.push(ToolClarificationField {
            key: "content".to_string(),
            label: "Message content".to_string(),
            description: "Provide the message body to send.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        });
    }
    if obj
        .get("channel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing_fields.push("channel".to_string());
        clarification_fields.push(ToolClarificationField {
            key: "channel".to_string(),
            label: "Target channel".to_string(),
            description: "Provide the explicit target channel for cross-target delivery."
                .to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        });
    }
    if obj
        .get("chat_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing_fields.push("chat_id".to_string());
        clarification_fields.push(ToolClarificationField {
            key: "chat_id".to_string(),
            label: "Target chat".to_string(),
            description: "Provide the explicit target chat id for cross-target delivery."
                .to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        });
    }
    if missing_fields.is_empty() {
        return None;
    }
    Some(
        ToolExecutionOutcome::text(message_blocked_payload(
            None,
            None,
            "supplemental",
            "message: missing required outbound fields",
        ))
        .with_blocker(ToolExecutionBlocker::needs_user_facts(
            "还需要补全要发送的内容和显式目标。",
            missing_fields,
            clarification_fields,
        )),
    )
}

fn invalid_delivery_kind_outcome(
    channel: Option<&str>,
    chat_id: Option<&str>,
    value: &str,
) -> Result<ToolExecutionOutcome> {
    Ok(
        ToolExecutionOutcome::text(message_blocked_payload(
            channel,
            chat_id,
            value,
            "message: delivery_kind must be supplemental or primary",
        ))
        .with_blocker(ToolExecutionBlocker::needs_user_choice(
            "请选择消息投递类型：supplemental 或 primary。",
            vec!["delivery_kind".to_string()],
            vec![ToolClarificationField {
                key: "delivery_kind".to_string(),
                label: "Delivery kind".to_string(),
                description:
                    "Choose supplemental for a visible update or primary for the canonical reply on the explicit target."
                        .to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: vec![
                    ToolClarificationOption {
                        value: "supplemental".to_string(),
                        label: "Supplemental".to_string(),
                    },
                    ToolClarificationOption {
                        value: "primary".to_string(),
                        label: "Primary".to_string(),
                    },
                ],
            }],
        )),
    )
}

fn runtime_blocked_message_outcome(
    channel: Option<&str>,
    chat_id: Option<&str>,
    delivery_kind: &str,
    summary: &str,
) -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(message_blocked_payload(
        channel,
        chat_id,
        delivery_kind,
        summary,
    ))
    .with_blocker(ToolExecutionBlocker::runtime_blocked(summary)))
}

fn unsupported_message_outcome(
    channel: Option<&str>,
    chat_id: Option<&str>,
    delivery_kind: &str,
    summary: &str,
) -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(message_blocked_payload(
        channel,
        chat_id,
        delivery_kind,
        summary,
    ))
    .with_blocker(ToolExecutionBlocker::unsupported(summary)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel_capability::{
        ChannelCapabilityContract, ChannelCapabilityEntry, ChannelDeliveryOrderingModel,
    };
    use crate::error::Error;
    use crate::platform::ResponseBody;
    use crate::tools::{ToolContext, ToolExecutionBlockerKind};
    use std::collections::HashMap;

    struct StubToolContext {
        current_channel: Option<String>,
        current_chat_id: Option<String>,
        channel_capabilities: HashMap<String, ChannelCapabilityEntry>,
        supports_current_chat_outbound_message: bool,
        supports_explicit_outbound_message: bool,
        outbound_message_budget: u8,
        outbound_message_count: u8,
    }

    impl ToolContext for StubToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn current_chat_id(&self) -> Option<&str> {
            self.current_chat_id.as_deref()
        }

        fn current_channel(&self) -> Option<&str> {
            self.current_channel.as_deref()
        }

        fn channel_capability(
            &self,
            channel: &str,
        ) -> Option<crate::channel_capability::ChannelCapabilityEntry> {
            self.channel_capabilities.get(channel).copied()
        }

        fn supports_current_chat_outbound_message(&self) -> bool {
            self.supports_current_chat_outbound_message
        }

        fn supports_explicit_outbound_message(&self) -> bool {
            self.supports_explicit_outbound_message
        }

        fn claim_outbound_message_delivery(
            &mut self,
            target_is_current: bool,
            primary: bool,
        ) -> Result<()> {
            if !target_is_current && !self.supports_explicit_outbound_message {
                return Err(Error::config(
                    "tool_message",
                    "explicit outbound target is not allowed",
                ));
            }
            if primary && target_is_current {
                return Err(Error::config(
                    "tool_message",
                    "current-chat primary reply is reserved for the canonical finalizer",
                ));
            }
            if self.outbound_message_count >= self.outbound_message_budget {
                return Err(Error::config(
                    "tool_message",
                    "outbound message budget exhausted",
                ));
            }
            self.outbound_message_count = self.outbound_message_count.saturating_add(1);
            Ok(())
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    fn capability_entry(
        id: &'static str,
        enabled: bool,
        supports_primary_reply: bool,
        supports_supplemental_reply: bool,
        supports_explicit_target: bool,
    ) -> ChannelCapabilityEntry {
        ChannelCapabilityEntry {
            id,
            configured: enabled,
            enabled,
            contract: ChannelCapabilityContract {
                supports_primary_reply,
                supports_supplemental_reply,
                supports_edit: false,
                supports_stream_edit: false,
                supports_explicit_target,
                supports_attachment: false,
                supports_typing_or_chat_action: false,
                supports_message_reaction: false,
                supported_body_kinds: &[
                    crate::bus::MessageBodyKind::Text,
                    crate::bus::MessageBodyKind::Card,
                    crate::bus::MessageBodyKind::PlatformNative,
                ],
                supported_text_formats: &[
                    crate::bus::TextFormat::Plain,
                    crate::bus::TextFormat::Markdown,
                    crate::bus::TextFormat::Html,
                    crate::bus::TextFormat::RichText,
                ],
                requires_pre_upload_for_media: false,
                supports_platform_handle_reuse: false,
                supports_http_url_media: false,
                requires_passive_reply_anchor: false,
                max_text_chars: Some(4096),
                max_caption_chars: None,
                delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
            },
        }
    }

    fn markdown_capability_entry(id: &'static str) -> ChannelCapabilityEntry {
        ChannelCapabilityEntry {
            id,
            configured: true,
            enabled: true,
            contract: ChannelCapabilityContract {
                supports_primary_reply: true,
                supports_supplemental_reply: true,
                supports_edit: false,
                supports_stream_edit: false,
                supports_explicit_target: true,
                supports_attachment: false,
                supports_typing_or_chat_action: false,
                supports_message_reaction: false,
                supported_body_kinds: &[crate::bus::MessageBodyKind::Text],
                supported_text_formats: &[
                    crate::bus::TextFormat::Plain,
                    crate::bus::TextFormat::Markdown,
                ],
                requires_pre_upload_for_media: false,
                supports_platform_handle_reuse: false,
                supports_http_url_media: false,
                requires_passive_reply_anchor: false,
                max_text_chars: Some(4096),
                max_caption_chars: None,
                delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
            },
        }
    }

    fn card_capability_entry(id: &'static str) -> ChannelCapabilityEntry {
        ChannelCapabilityEntry {
            id,
            configured: true,
            enabled: true,
            contract: ChannelCapabilityContract {
                supports_primary_reply: true,
                supports_supplemental_reply: true,
                supports_edit: false,
                supports_stream_edit: false,
                supports_explicit_target: true,
                supports_attachment: true,
                supports_typing_or_chat_action: false,
                supports_message_reaction: false,
                supported_body_kinds: &[
                    crate::bus::MessageBodyKind::Text,
                    crate::bus::MessageBodyKind::Card,
                ],
                supported_text_formats: &[crate::bus::TextFormat::Plain],
                requires_pre_upload_for_media: false,
                supports_platform_handle_reuse: false,
                supports_http_url_media: false,
                requires_passive_reply_anchor: false,
                max_text_chars: Some(4096),
                max_caption_chars: None,
                delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
            },
        }
    }

    fn capability_map(
        entries: &[ChannelCapabilityEntry],
    ) -> HashMap<String, ChannelCapabilityEntry> {
        entries
            .iter()
            .copied()
            .map(|entry| (entry.id.to_string(), entry))
            .collect()
    }

    #[test]
    fn current_chat_target_is_ignored_for_canonical_reply_surface() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[capability_entry(
                "qq_channel",
                true,
                true,
                true,
                true,
            )]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"done","channel":"qq_channel","chat_id":"chat-1","delivery_kind":"primary"}"#,
                &mut ctx,
            )
            .expect("current-chat noop");

        assert!(outcome.blocker.is_none());
        assert!(outcome.outbound_intents.is_empty());
        assert!(outcome.is_success());
        assert_eq!(ctx.outbound_message_count, 0);
    }

    #[test]
    fn supplemental_explicit_message_does_not_mark_current_chat_reply() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                markdown_capability_entry("telegram"),
            ]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"ping","channel":"telegram","chat_id":"chat-2","delivery_kind":"supplemental"}"#,
                &mut ctx,
            )
            .expect("message tool");

        assert_eq!(
            outcome.outbound_intents.as_slice(),
            &[ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit {
                    channel: "telegram".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "ping".to_string(),
                body: Some(CanonicalMessageBody::Text(TextBody {
                    text: "ping".to_string(),
                    format: TextFormat::Plain,
                })),
            }]
        );
    }

    #[test]
    fn markdown_explicit_message_builds_rich_text_body() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                markdown_capability_entry("telegram"),
            ]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"**ping**","channel":"telegram","chat_id":"chat-2","text_format":"markdown"}"#,
                &mut ctx,
            )
            .expect("markdown message tool");

        assert_eq!(
            outcome.outbound_intents.as_slice(),
            &[ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit {
                    channel: "telegram".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "**ping**".to_string(),
                body: Some(CanonicalMessageBody::Text(TextBody {
                    text: "**ping**".to_string(),
                    format: TextFormat::Markdown,
                })),
            }]
        );
    }

    #[test]
    fn card_explicit_message_builds_card_body_without_text_content() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                card_capability_entry("feishu"),
            ]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"channel":"feishu","chat_id":"chat-2","body_kind":"card","payload_json":{"header":{"title":"Build passed"}}}"#,
                &mut ctx,
            )
            .expect("card message tool");

        assert_eq!(
            outcome.outbound_intents.as_slice(),
            &[ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit {
                    channel: "feishu".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "[card]".to_string(),
                body: Some(CanonicalMessageBody::Card(CardBody {
                    format: crate::bus::CardFormat::Interactive,
                    payload_json: serde_json::json!({"header":{"title":"Build passed"}}),
                    fallback_text: String::new(),
                })),
            }]
        );
    }

    #[test]
    fn explicit_current_chat_target_is_ignored_before_runtime_delivery_permissions() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[capability_entry(
                "qq_channel",
                true,
                true,
                true,
                true,
            )]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"done","channel":"qq_channel","chat_id":"chat-1","delivery_kind":"primary"}"#,
                &mut ctx,
            )
            .expect("current-chat noop");
        assert!(outcome.blocker.is_none());
        assert!(outcome.outbound_intents.is_empty());
        assert!(outcome.is_success());
        assert_eq!(ctx.outbound_message_count, 0);
    }

    #[test]
    fn explicit_message_rejects_runtime_without_explicit_permission() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                capability_entry("telegram", true, true, true, true),
            ]),
            supports_current_chat_outbound_message: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"ping","channel":"telegram","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect("runtime blocker");
        let blocker = outcome.blocker.expect("runtime blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::RuntimeBlocked);
    }

    #[test]
    fn message_tool_enforces_per_turn_budget() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                capability_entry("telegram", true, true, true, true),
            ]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 1,
            outbound_message_count: 0,
        };
        let tool = MessageTool;

        tool.execute_outcome(
            r#"{"content":"first","channel":"telegram","chat_id":"chat-2","delivery_kind":"primary"}"#,
            &mut ctx,
        )
            .expect("first message");
        let outcome = tool
            .execute_outcome(
                r#"{"content":"second","channel":"telegram","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect("second message should block");

        let blocker = outcome.blocker.expect("runtime blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::RuntimeBlocked);
    }

    #[test]
    fn message_tool_requires_explicit_target_fields() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[capability_entry(
                "qq_channel",
                true,
                true,
                true,
                true,
            )]),
            supports_current_chat_outbound_message: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"status","delivery_kind":"supplemental"}"#,
                &mut ctx,
            )
            .expect("explicit target blocker");

        let blocker = outcome.blocker.expect("target blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert_eq!(
            blocker.missing_fields,
            vec!["channel".to_string(), "chat_id".to_string()]
        );
        assert_eq!(ctx.outbound_message_count, 0);
    }

    #[test]
    fn explicit_message_rejects_channel_without_explicit_target_contract() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                capability_entry("dingtalk", true, true, true, false),
            ]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"ping","channel":"dingtalk","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect("explicit contract blocker");

        let blocker = outcome.blocker.expect("unsupported blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::Unsupported);
    }

    #[test]
    fn message_tool_requires_content_blocker() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[capability_entry(
                "telegram", true, true, true, true,
            )]),
            supports_current_chat_outbound_message: false,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(r#"{"channel":"telegram","chat_id":"chat-2"}"#, &mut ctx)
            .expect("content blocker");

        let blocker = outcome.blocker.expect("content blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert_eq!(blocker.missing_fields, vec!["content".to_string()]);
    }

    #[test]
    fn message_tool_invalid_delivery_kind_requests_choice() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[capability_entry(
                "telegram", true, true, true, true,
            )]),
            supports_current_chat_outbound_message: false,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"ping","channel":"telegram","chat_id":"chat-2","delivery_kind":"weird"}"#,
                &mut ctx,
            )
            .expect("delivery kind blocker");

        let blocker = outcome.blocker.expect("choice blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
        assert_eq!(blocker.missing_fields, vec!["delivery_kind".to_string()]);
    }

    #[test]
    fn explicit_message_runtime_without_explicit_permission_returns_runtime_blocker() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                capability_entry("telegram", true, true, true, true),
            ]),
            supports_current_chat_outbound_message: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"ping","channel":"telegram","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect("runtime blocker");

        let blocker = outcome.blocker.expect("runtime blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::RuntimeBlocked);
    }

    #[test]
    fn message_tool_budget_exhaustion_returns_runtime_blocker() {
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            channel_capabilities: capability_map(&[
                capability_entry("qq_channel", true, true, true, true),
                capability_entry("telegram", true, true, true, true),
            ]),
            supports_current_chat_outbound_message: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 1,
            outbound_message_count: 0,
        };
        let tool = MessageTool;

        tool.execute_outcome(
            r#"{"content":"first","channel":"telegram","chat_id":"chat-2","delivery_kind":"primary"}"#,
            &mut ctx,
        )
        .expect("first message");
        let outcome = tool
            .execute_outcome(
                r#"{"content":"second","channel":"telegram","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect("budget blocker");

        let blocker = outcome.blocker.expect("runtime blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::RuntimeBlocked);
    }
}

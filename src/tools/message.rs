use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolExecutionOutcome, ToolMetadata};
use serde_json::{json, Value};

pub struct MessageTool;

impl Tool for MessageTool {
    fn name(&self) -> &'static str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a user-visible message through the runtime outbound pipeline. Use target=current for the active chat, or target=explicit with channel and chat_id."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Message body to send."
                },
                "target": {
                    "type": "string",
                    "enum": ["current", "explicit"],
                    "description": "current = active chat; explicit = use channel + chat_id.",
                    "default": "current"
                },
                "channel": {
                    "type": "string",
                    "description": "Required when target=explicit."
                },
                "chat_id": {
                    "type": "string",
                    "description": "Required when target=explicit."
                },
                "delivery_kind": {
                    "type": "string",
                    "enum": ["supplemental", "primary"],
                    "description": "primary means this tool message is the main reply for the current chat.",
                    "default": "supplemental"
                }
            },
            "required": ["content"]
        })
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
        let content = obj
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("tool_message", "missing content"))?;

        let target = obj
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("current");
        let delivery_kind = obj
            .get("delivery_kind")
            .and_then(Value::as_str)
            .unwrap_or("supplemental");
        let (channel, chat_id, current_target) = match target {
            "current" => {
                let channel = ctx.current_channel().ok_or_else(|| {
                    Error::config("tool_message", "current channel is unavailable")
                })?;
                let chat_id = ctx.current_chat_id().ok_or_else(|| {
                    Error::config("tool_message", "current chat_id is unavailable")
                })?;
                (channel.to_string(), chat_id.to_string(), true)
            }
            "explicit" => {
                let channel = obj
                    .get("channel")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        Error::config("tool_message", "missing channel for explicit target")
                    })?;
                let chat_id = obj
                    .get("chat_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        Error::config("tool_message", "missing chat_id for explicit target")
                    })?;
                let current_target = ctx.current_channel() == Some(channel)
                    && ctx.current_chat_id() == Some(chat_id);
                (channel.to_string(), chat_id.to_string(), current_target)
            }
            _ => {
                return Err(Error::config(
                    "tool_message",
                    "target must be current or explicit",
                ));
            }
        };

        let primary = match delivery_kind {
            "supplemental" => false,
            "primary" => true,
            _ => {
                return Err(Error::config(
                    "tool_message",
                    "delivery_kind must be supplemental or primary",
                ));
            }
        };
        if primary && current_target && !ctx.supports_current_chat_primary_reply() {
            return Err(Error::config(
                "tool_message",
                "primary current-chat delivery is not supported in this runtime context",
            ));
        }
        if !current_target && !ctx.supports_explicit_outbound_message() {
            return Err(Error::config(
                "tool_message",
                "explicit outbound target is not allowed in this runtime context",
            ));
        }
        ctx.claim_outbound_message_delivery(current_target, primary)?;

        let summary = json!({
            "ok": true,
            "tool": "message",
            "target": if current_target { "current" } else { "explicit" },
            "delivery_kind": if primary { "primary" } else { "supplemental" },
            "channel": channel,
            "chat_id": chat_id,
            "sent_chars": content.chars().count(),
            "delivered_by_runtime": primary && current_target,
        })
        .to_string();

        if !(primary && current_target) {
            ctx.send_outbound_message(&channel, &chat_id, content)?;
        }

        let outcome = ToolExecutionOutcome::text(summary);
        if primary && current_target {
            Ok(outcome.with_current_chat_reply(content))
        } else {
            Ok(outcome)
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;
    use crate::platform::ResponseBody;
    use crate::tools::ToolContext;

    struct StubToolContext {
        current_channel: Option<String>,
        current_chat_id: Option<String>,
        supports_current_chat_primary_reply: bool,
        supports_explicit_outbound_message: bool,
        outbound_message_budget: u8,
        outbound_message_count: u8,
        current_primary_message_delivered: bool,
        outbound_tx: crate::bus::OutboundTx,
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

        fn supports_current_chat_primary_reply(&self) -> bool {
            self.supports_current_chat_primary_reply
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
                if self.current_primary_message_delivered {
                    return Err(Error::config(
                        "tool_message",
                        "current primary already claimed",
                    ));
                }
                self.current_primary_message_delivered = true;
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

        fn send_outbound_message(
            &mut self,
            channel: &str,
            chat_id: &str,
            content: &str,
        ) -> Result<()> {
            let msg = crate::bus::PcMsg::new(channel, chat_id, content)?;
            self.outbound_tx
                .try_send(msg)
                .map_err(|e| Error::config("tool_message_test", e.to_string()))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn primary_current_message_marks_current_chat_reply() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(4);
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            supports_current_chat_primary_reply: true,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
            current_primary_message_delivered: false,
            outbound_tx,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(r#"{"content":"done","delivery_kind":"primary"}"#, &mut ctx)
            .expect("message tool");

        assert_eq!(
            outcome
                .current_chat_reply
                .as_ref()
                .map(|reply| reply.content.as_str()),
            Some("done")
        );
        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn supplemental_explicit_message_does_not_mark_current_chat_reply() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(4);
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            supports_current_chat_primary_reply: false,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 2,
            outbound_message_count: 0,
            current_primary_message_delivered: false,
            outbound_tx,
        };
        let tool = MessageTool;
        let outcome = tool
            .execute_outcome(
                r#"{"content":"ping","target":"explicit","channel":"telegram","chat_id":"chat-2","delivery_kind":"supplemental"}"#,
                &mut ctx,
            )
            .expect("message tool");

        assert!(outcome.current_chat_reply.is_none());
        let outbound = outbound_rx.try_recv().expect("outbound");
        assert_eq!(outbound.channel.as_ref(), "telegram");
        assert_eq!(outbound.chat_id.as_ref(), "chat-2");
        assert_eq!(outbound.content, "ping");
    }

    #[test]
    fn primary_current_message_rejects_unsupported_runtime() {
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            supports_current_chat_primary_reply: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
            current_primary_message_delivered: false,
            outbound_tx,
        };
        let tool = MessageTool;
        let err = tool
            .execute_outcome(r#"{"content":"done","delivery_kind":"primary"}"#, &mut ctx)
            .expect_err("primary current message should fail");
        assert_eq!(err.stage(), "tool_message");
    }

    #[test]
    fn explicit_message_rejects_runtime_without_explicit_permission() {
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            supports_current_chat_primary_reply: false,
            supports_explicit_outbound_message: false,
            outbound_message_budget: 2,
            outbound_message_count: 0,
            current_primary_message_delivered: false,
            outbound_tx,
        };
        let tool = MessageTool;
        let err = tool
            .execute_outcome(
                r#"{"content":"ping","target":"explicit","channel":"telegram","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect_err("explicit message should fail");
        assert_eq!(err.stage(), "tool_message");
    }

    #[test]
    fn message_tool_enforces_per_turn_budget() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(4);
        let mut ctx = StubToolContext {
            current_channel: Some("qq_channel".to_string()),
            current_chat_id: Some("chat-1".to_string()),
            supports_current_chat_primary_reply: true,
            supports_explicit_outbound_message: true,
            outbound_message_budget: 1,
            outbound_message_count: 0,
            current_primary_message_delivered: false,
            outbound_tx,
        };
        let tool = MessageTool;

        tool.execute_outcome(r#"{"content":"first","delivery_kind":"primary"}"#, &mut ctx)
            .expect("first message");
        let err = tool
            .execute_outcome(
                r#"{"content":"second","target":"explicit","channel":"telegram","chat_id":"chat-2"}"#,
                &mut ctx,
            )
            .expect_err("second message should hit budget");

        assert_eq!(err.stage(), "tool_message");
        assert!(outbound_rx.try_recv().is_err());
    }
}

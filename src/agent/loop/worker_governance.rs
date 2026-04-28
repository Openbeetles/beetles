use super::*;

pub(super) fn maybe_apply_mental_privacy_review(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
    loc: UiLocale,
    reply_surface: ReplySurface,
    reply_content: String,
    worker_latency: &mut WorkerLatency,
) -> MentalPrivacyReviewOutcome {
    if msg.ingress != IngressKind::User || reply_content.trim().is_empty() {
        return MentalPrivacyReviewOutcome {
            reply_content,
            action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
            applied: false,
            touched_targets: Vec::new(),
        };
    }
    if !reply_surface.allows_mental_privacy_review() {
        return MentalPrivacyReviewOutcome {
            reply_content,
            action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
            applied: false,
            touched_targets: Vec::new(),
        };
    }

    let t0 = metrics::record_llm_call_start();
    let review_started = Instant::now();
    let mut privacy_http = HttpClientToolContext {
        http,
        chat_id: Some(msg.chat_id.clone()),
        ingress: msg.ingress,
        channel: Some(msg.channel.clone()),
        tool_registry: None,
        channel_capability_registry: Arc::clone(&config.channel_capability_registry),
        supports_current_chat_outbound_message: false,
        supports_explicit_outbound_message: false,
        outbound_message_budget: 0,
        outbound_message_count: 0,
        locale: loc,
    };
    match run_mental_privacy_review(
        &mut privacy_http,
        worker_llm,
        MentalPrivacyReviewContext {
            mental_privacy_store: config.runtime.mental_privacy_store.as_ref(),
            relationship_constitution_store: config
                .runtime
                .relationship_constitution_store
                .as_ref(),
            self_model_store: config.runtime.self_model_store.as_ref(),
            self_continuity_store: config.runtime.self_continuity_store.as_ref(),
            inner_life_store: config.runtime.inner_life_store.as_ref(),
            private_doc_store: config.runtime.private_doc_store.as_ref(),
            private_garden_store: config.runtime.private_garden_store.as_ref(),
        },
        MentalPrivacyReviewInput {
            channel: &msg.channel,
            chat_id: &msg.chat_id,
            user_content: &msg.content,
            draft_reply: &reply_content,
            now_secs: crate::util::current_unix_secs(),
        },
    ) {
        Ok(review) => {
            metrics::record_llm_call_end(t0);
            worker_latency.mental_privacy_review_ms = worker_latency
                .mental_privacy_review_ms
                .saturating_add(review_started.elapsed().as_millis());
            review
        }
        Err(error) => {
            metrics::record_llm_call_end(t0);
            metrics::record_llm_error();
            worker_latency.mental_privacy_review_ms = worker_latency
                .mental_privacy_review_ms
                .saturating_add(review_started.elapsed().as_millis());
            log::warn!("[agent_mental_privacy] review failed: {}", error);
            MentalPrivacyReviewOutcome {
                reply_content: mental_privacy_review_failure_reply(loc).to_string(),
                action: crate::memory::MentalPrivacyShareAction::Defer,
                applied: true,
                touched_targets: Vec::new(),
            }
        }
    }
}

fn mental_privacy_review_failure_reply(loc: UiLocale) -> &'static str {
    match loc {
        UiLocale::Zh => {
            "这次触及到私域边界，但我现在没法可靠完成隐私审查，所以先不公开这些内容。"
        }
        UiLocale::En => {
            "This touches a private boundary, and I cannot complete the privacy review reliably right now, so I will not disclose it."
        }
    }
}

fn turn_persona_scope_from_share_action(
    action: crate::memory::MentalPrivacyShareAction,
) -> &'static str {
    match action {
        crate::memory::MentalPrivacyShareAction::Refuse => "refuse",
        crate::memory::MentalPrivacyShareAction::Defer => "defer",
        crate::memory::MentalPrivacyShareAction::AllowSummary
        | crate::memory::MentalPrivacyShareAction::AllowRedactedExcerpt
        | crate::memory::MentalPrivacyShareAction::ExplainWithoutQuote => "narrow",
        crate::memory::MentalPrivacyShareAction::AllowRaw => "brief",
        crate::memory::MentalPrivacyShareAction::AllowOriginal => "full",
    }
}

fn derive_turn_persona_reply_scope(
    is_interrupt: bool,
    priority: Option<&PersonaPriorityAdjudication>,
    disclosure: Option<&crate::memory::MentalPrivacyDisclosureAdjudication>,
    review: &MentalPrivacyReviewOutcome,
) -> String {
    if is_interrupt {
        return "interrupt".to_string();
    }
    let scope = priority
        .map(|priority| priority.task_scope.trim())
        .filter(|scope| !scope.is_empty())
        .map(str::to_string)
        .or_else(|| {
            disclosure.map(|adjudication| {
                turn_persona_scope_from_share_action(adjudication.share_action).to_string()
            })
        })
        .or_else(|| {
            review
                .applied
                .then(|| turn_persona_scope_from_share_action(review.action).to_string())
        })
        .unwrap_or_else(|| "full".to_string());
    normalize_turn_persona_scope(&scope)
}

fn build_turn_persona_targets(
    disclosure: Option<&crate::memory::MentalPrivacyDisclosureAdjudication>,
    review: &MentalPrivacyReviewOutcome,
) -> Vec<String> {
    let mut targets = disclosure
        .map(|adjudication| adjudication.targets.clone())
        .unwrap_or_default();
    targets.extend(review.touched_targets.iter().cloned());
    normalize_turn_persona_targets(&targets)
}

pub(super) fn build_turn_persona_ledger(
    pressure: crate::orchestrator::PressureLevel,
    tool_calls: u32,
    delivered: bool,
    is_interrupt: bool,
    disclosure: Option<&crate::memory::MentalPrivacyDisclosureAdjudication>,
    priority: Option<&PersonaPriorityAdjudication>,
    review: &MentalPrivacyReviewOutcome,
    review_rewrite_applied: bool,
) -> Option<TurnPersonaLedger> {
    let persona = TurnPersonaLedger {
        disclosure: disclosure.map(build_turn_persona_disclosure_ledger),
        priority: priority.map(build_turn_persona_priority_ledger),
        review: TurnPersonaReviewLedger {
            action: review.action,
            applied: review.applied,
            rewrite_applied: review_rewrite_applied,
        },
        touched_targets: build_turn_persona_targets(disclosure, review),
        pressure: pressure.into(),
        tool_calls,
        reply_scope: derive_turn_persona_reply_scope(is_interrupt, priority, disclosure, review),
        reply_delivered: delivered,
    };
    persona.is_meaningful().then_some(persona)
}

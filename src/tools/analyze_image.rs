//! analyze_image 工具：通过 LLM 多模态能力分析图片 URL 或真实 platform camera 帧。
//! 支持多源顺序回退，与 FallbackLlmClient 降级行为一致。
//! analyze_image tool: analyzes image URLs or real platform camera frames via LLM vision.
//! Supports multi-source fallback, consistent with FallbackLlmClient.

use crate::config::{LlmModelKind, LlmSource};
use crate::constants::MAX_REQUEST_BODY_LEN;
use crate::error::{Error, Result};
use crate::llm::LlmRequestBody;
use crate::platform::{CameraFrameBuffer, CameraFrameFormat, PlatformCamera};
use crate::runtime::frame_lease::{try_acquire_frame_capture_permit, MAX_CAMERA_CAPTURE_BYTES};
#[cfg(test)]
use crate::runtime::frame_lease::{
    try_acquire_frame_capture_permit_with_admission, FrameLeaseAdmission,
};
use crate::runtime::lease::LeaseOwner;
use crate::runtime::RuntimeWorkSource;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolEffectClass, ToolMetadata};
use base64::Engine as _;
use std::sync::Arc;

const TAG: &str = "tools::analyze_image";
const STAGE: &str = "tool_analyze_image";
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
const VISION_MAX_TOKENS: u32 = 1024;

pub struct AnalyzeImageTool {
    sources: Vec<LlmSource>,
    camera: Option<Arc<dyn PlatformCamera>>,
    #[cfg(test)]
    camera_admission_override: Option<FrameLeaseAdmission>,
}

impl AnalyzeImageTool {
    pub fn new(config: &crate::config::AppConfig) -> Self {
        Self::from_sources_and_camera(config, None)
    }

    pub fn with_camera(config: &crate::config::AppConfig, camera: Arc<dyn PlatformCamera>) -> Self {
        Self::from_sources_and_camera(config, Some(camera))
    }

    pub fn with_platform_camera(
        config: &crate::config::AppConfig,
        platform: Arc<dyn crate::platform::Platform>,
    ) -> Self {
        Self::with_camera(config, Arc::new(PlatformCameraAdapter { platform }))
    }

    fn from_sources_and_camera(
        config: &crate::config::AppConfig,
        camera: Option<Arc<dyn PlatformCamera>>,
    ) -> Self {
        let sources: Vec<LlmSource> = config
            .llm_sources
            .iter()
            .filter(|s| {
                s.model_kind == LlmModelKind::Multimodal
                    && !s.api_key.trim().is_empty()
                    && !s.model.trim().is_empty()
                    && !s.provider.trim().is_empty()
            })
            .cloned()
            .collect();
        Self {
            sources,
            camera,
            #[cfg(test)]
            camera_admission_override: None,
        }
    }

    #[cfg(test)]
    fn with_camera_and_admission_for_tests(
        config: &crate::config::AppConfig,
        camera: Arc<dyn PlatformCamera>,
        admission: FrameLeaseAdmission,
    ) -> Self {
        let mut tool = Self::with_camera(config, camera);
        tool.camera_admission_override = Some(admission);
        tool
    }

    /// 用单个源执行一次 vision 请求，返回 Ok(text) 或 Err。
    fn try_source(
        source: &LlmSource,
        image: VisionImageInput<'_>,
        question: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<String> {
        let is_anthropic = source.provider == "anthropic";

        let body = if is_anthropic {
            Self::build_anthropic_request_body(&source.model, image, question)
        } else {
            Self::build_openai_request_body(&source.model, image, question)
        }
        .map_err(map_vision_body_error)?;

        let url = if is_anthropic {
            if source.api_url.trim().is_empty() {
                ANTHROPIC_API_URL.to_string()
            } else {
                source.api_url.trim_end_matches('/').to_string()
            }
        } else {
            let base = if source.api_url.trim().is_empty() {
                OPENAI_DEFAULT_API_BASE
            } else {
                source.api_url.trim_end_matches('/')
            };
            format!("{}/chat/completions", base)
        };

        let bearer;
        let mut headers: Vec<(&str, &str)> = if is_anthropic {
            vec![
                ("Content-Type", "application/json"),
                ("x-api-key", &source.api_key),
                ("anthropic-version", "2023-06-01"),
            ]
        } else {
            bearer = format!("Bearer {}", source.api_key);
            vec![
                ("Content-Type", "application/json"),
                ("Authorization", &bearer),
            ]
        };
        crate::llm::append_non_overriding_custom_headers(&mut headers, &source.custom_headers);

        log::info!(
            "[{}] POST {} provider={} model={} image_url_len={}",
            TAG,
            url,
            source.provider,
            source.model,
            image.log_len()
        );

        let (status, resp_body) = ctx.post_with_headers(&url, &headers, body.as_ref())?;

        if status >= 400 {
            let err_bytes = resp_body.as_ref();
            let preview_len = err_bytes.len().min(256);
            let err_text = String::from_utf8_lossy(&err_bytes[..preview_len]);
            log::warn!("[{}] API returned status={}: {}", TAG, status, err_text);
            return Err(Error::Http {
                status_code: status,
                stage: STAGE,
            });
        }

        let parsed: serde_json::Value =
            serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: STAGE,
            })?;

        let text = if is_anthropic {
            Self::extract_anthropic_text(&parsed)
        } else {
            Self::extract_openai_text(&parsed)
        };

        match text {
            Some(t) => {
                log::info!("[{}] result len={}", TAG, t.len());
                Ok(t)
            }
            None => {
                log::warn!("[{}] failed to extract text from response", TAG);
                Ok("analyze_image: could not extract text from API response".to_string())
            }
        }
    }

    fn build_anthropic_request_body(
        model: &str,
        image: VisionImageInput<'_>,
        question: &str,
    ) -> Result<LlmRequestBody> {
        let mut body = LlmRequestBody::with_estimated_capacity(
            vision_request_estimated_capacity(model, image, question, true),
            MAX_REQUEST_BODY_LEN,
        );
        body.push_byte(b'{')?;
        body.push_json_string_field("model", model)?;
        body.push_str(",\"max_tokens\":")?;
        push_u32_decimal(&mut body, VISION_MAX_TOKENS)?;
        body.push_str(",\"messages\":[{\"role\":\"user\",\"content\":[")?;
        body.push_str("{\"type\":\"image\",\"source\":")?;
        match image {
            VisionImageInput::Url(image_url) => {
                body.push_byte(b'{')?;
                body.push_json_string_field("type", "url")?;
                body.push_byte(b',')?;
                body.push_json_string_field("url", image_url)?;
                body.push_byte(b'}')?;
            }
            VisionImageInput::LocalFrame { media_type, bytes } => {
                body.push_byte(b'{')?;
                body.push_json_string_field("type", "base64")?;
                body.push_byte(b',')?;
                body.push_json_string_field("media_type", media_type)?;
                body.push_str(",\"data\":\"")?;
                push_base64_standard(&mut body, bytes)?;
                body.push_str("\"}")?;
            }
        }
        body.push_str("},{\"type\":\"text\",\"text\":")?;
        body.push_json_string(question)?;
        body.push_str("}]}]}")?;
        body.finish(MAX_REQUEST_BODY_LEN)
    }

    fn build_openai_request_body(
        model: &str,
        image: VisionImageInput<'_>,
        question: &str,
    ) -> Result<LlmRequestBody> {
        let mut body = LlmRequestBody::with_estimated_capacity(
            vision_request_estimated_capacity(model, image, question, false),
            MAX_REQUEST_BODY_LEN,
        );
        body.push_byte(b'{')?;
        body.push_json_string_field("model", model)?;
        body.push_str(",\"max_tokens\":")?;
        push_u32_decimal(&mut body, VISION_MAX_TOKENS)?;
        body.push_str(",\"messages\":[{\"role\":\"user\",\"content\":[")?;
        body.push_str("{\"type\":\"image_url\",\"image_url\":{\"url\":")?;
        match image {
            VisionImageInput::Url(image_url) => {
                body.push_json_string(image_url)?;
            }
            VisionImageInput::LocalFrame { media_type, bytes } => {
                body.push_str("\"data:")?;
                body.push_str(media_type)?;
                body.push_str(";base64,")?;
                push_base64_standard(&mut body, bytes)?;
                body.push_byte(b'"')?;
            }
        }
        body.push_str("}},{\"type\":\"text\",\"text\":")?;
        body.push_json_string(question)?;
        body.push_str("}]}]}")?;
        body.finish(MAX_REQUEST_BODY_LEN)
    }

    fn extract_anthropic_text(resp: &serde_json::Value) -> Option<String> {
        resp.get("content")?
            .as_array()?
            .iter()
            .filter_map(|block| {
                if block.get("type")?.as_str()? == "text" {
                    block.get("text")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .next()
    }

    fn extract_openai_text(resp: &serde_json::Value) -> Option<String> {
        resp.get("choices")?
            .as_array()?
            .first()?
            .get("message")?
            .get("content")?
            .as_str()
            .map(String::from)
    }

    fn execute_url_vision(
        &self,
        image_url: &str,
        question: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<String> {
        if !image_url.starts_with("http://") && !image_url.starts_with("https://") {
            return Err(Error::config(
                STAGE,
                "image_url must start with http:// or https://",
            ));
        }
        if image_url.len() > 2048 {
            return Err(Error::config(STAGE, "image_url too long (max 2048)"));
        }
        self.execute_vision_input(VisionImageInput::Url(image_url), question, ctx)
    }

    fn execute_local_camera(
        &self,
        max_bytes: usize,
        question: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<String> {
        if max_bytes > MAX_CAMERA_CAPTURE_BYTES {
            return Err(Error::config(
                "camera_frame_admission",
                "camera_frame_max_bytes_exceeded",
            ));
        }
        let camera = self
            .camera
            .as_ref()
            .ok_or_else(|| Error::config(STAGE, "camera_unavailable"))?;
        let status = camera.camera_status();
        if !status.is_available() {
            return Err(Error::config(STAGE, "camera_unavailable"));
        }
        if let Some(platform_max) = status.max_frame_bytes {
            if max_bytes > platform_max {
                return Err(Error::config(STAGE, "camera_platform_max_bytes_exceeded"));
            }
        }

        if self.sources.is_empty() {
            return Ok("analyze_image: no multimodal model configured".to_string());
        }

        let owner = LeaseOwner::new("camera_vision", "analyze_image");
        #[cfg(test)]
        let _permit = match self.camera_admission_override {
            Some(admission) => {
                try_acquire_frame_capture_permit_with_admission(owner, max_bytes, admission)
            }
            None => {
                try_acquire_frame_capture_permit(owner, max_bytes, RuntimeWorkSource::UserFacing)
            }
        }?;
        #[cfg(not(test))]
        let _permit =
            try_acquire_frame_capture_permit(owner, max_bytes, RuntimeWorkSource::UserFacing)?;
        let frame = camera.capture_frame(max_bytes)?;
        if frame.bytes.len() > max_bytes {
            return Err(Error::config(STAGE, "camera_frame_exceeds_requested_max"));
        }
        if !matches!(
            frame.format,
            CameraFrameFormat::Jpeg | CameraFrameFormat::Png
        ) {
            return Err(Error::config(STAGE, "unsupported_camera_frame_format"));
        }
        if !local_frame_fits_request_budget(frame.bytes.len(), question.len(), MAX_REQUEST_BODY_LEN)
        {
            return Err(Error::config(STAGE, "vision_request_body_too_large"));
        }
        self.execute_vision_input(
            VisionImageInput::LocalFrame {
                media_type: frame.format.media_type(),
                bytes: frame.bytes.as_ref(),
            },
            question,
            ctx,
        )
    }

    fn execute_vision_input(
        &self,
        image: VisionImageInput<'_>,
        question: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<String> {
        if self.sources.is_empty() {
            return Ok("analyze_image: no multimodal model configured".to_string());
        }

        // 多源顺序回退，与 FallbackLlmClient 行为一致
        let mut last_err = None;
        for (i, source) in self.sources.iter().enumerate() {
            match Self::try_source(source, image, question, ctx) {
                Ok(text) => return Ok(text),
                Err(e) => {
                    if i + 1 < self.sources.len() {
                        log::warn!(
                            "[{}] source {} ({}/{}) failed, trying next: {}",
                            TAG,
                            i,
                            source.provider,
                            source.model,
                            e
                        );
                    }
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| Error::config(STAGE, "all sources failed")))
    }
}

fn base64_encoded_len(bytes: usize) -> Option<usize> {
    bytes.checked_add(2)?.checked_div(3)?.checked_mul(4)
}

fn vision_request_estimated_capacity(
    model: &str,
    image: VisionImageInput<'_>,
    question: &str,
    is_anthropic: bool,
) -> usize {
    const VISION_JSON_OVERHEAD_BYTES: usize = 1024;
    let image_bytes = match image {
        VisionImageInput::Url(image_url) => image_url.len(),
        VisionImageInput::LocalFrame { media_type, bytes } => base64_encoded_len(bytes.len())
            .unwrap_or(MAX_REQUEST_BODY_LEN)
            .saturating_add(media_type.len())
            .saturating_add(if is_anthropic {
                0
            } else {
                "data:;base64,".len()
            }),
    };
    model
        .len()
        .saturating_add(question.len())
        .saturating_add(image_bytes)
        .saturating_add(VISION_JSON_OVERHEAD_BYTES)
}

fn local_frame_fits_request_budget(
    frame_bytes: usize,
    question_bytes: usize,
    budget: usize,
) -> bool {
    const LOCAL_FRAME_JSON_OVERHEAD_BYTES: usize = 4096;
    base64_encoded_len(frame_bytes)
        .and_then(|encoded| encoded.checked_add(question_bytes))
        .and_then(|with_question| with_question.checked_add(LOCAL_FRAME_JSON_OVERHEAD_BYTES))
        .is_some_and(|estimate| estimate <= budget)
}

fn push_u32_decimal(out: &mut LlmRequestBody, value: u32) -> Result<()> {
    let mut num_buf = [0u8; 20];
    out.push_str(crate::util::usize_to_decimal_buf(
        &mut num_buf,
        value as usize,
    ))
}

fn push_base64_standard(out: &mut LlmRequestBody, bytes: &[u8]) -> Result<()> {
    const RAW_CHUNK: usize = 3 * 256;
    const ENCODED_CHUNK: usize = RAW_CHUNK / 3 * 4;
    let mut encoded = [0u8; ENCODED_CHUNK];
    for chunk in bytes.chunks(RAW_CHUNK) {
        let len = base64::engine::general_purpose::STANDARD
            .encode_slice(chunk, &mut encoded)
            .map_err(|_| Error::config(STAGE, "base64_encode_failed"))?;
        out.push_bytes(&encoded[..len])?;
    }
    Ok(())
}

fn map_vision_body_error(err: Error) -> Error {
    match err {
        Error::Config { stage, message }
            if stage == "llm_request" && message.contains("request body exceeds") =>
        {
            Error::config(STAGE, "vision_request_body_too_large")
        }
        other => other,
    }
}

struct PlatformCameraAdapter {
    platform: Arc<dyn crate::platform::Platform>,
}

impl PlatformCamera for PlatformCameraAdapter {
    fn camera_status(&self) -> crate::platform::CameraStatus {
        self.platform.camera_status()
    }

    fn capture_frame(&self, max_bytes: usize) -> Result<CameraFrameBuffer> {
        self.platform.capture_frame(max_bytes)
    }
}

#[derive(Clone, Copy)]
enum VisionImageInput<'a> {
    Url(&'a str),
    LocalFrame {
        media_type: &'static str,
        bytes: &'a [u8],
    },
}

impl VisionImageInput<'_> {
    fn log_len(self) -> usize {
        match self {
            Self::Url(image_url) => image_url.len(),
            Self::LocalFrame { bytes, .. } => {
                base64_encoded_len(bytes.len()).unwrap_or(bytes.len())
            }
        }
    }
}

impl Tool for AnalyzeImageTool {
    fn name(&self) -> &'static str {
        "analyze_image"
    }

    fn description(&self) -> &'static str {
        "Analyze an image from a URL using vision AI. Use this when a user sends an image URL and you need to understand its content."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"source":{"type":"string","enum":["url","camera"],"description":"Use url for HTTP/HTTPS image URLs, or camera for a local platform camera capture"},"image_url":{"type":"string","description":"The HTTP/HTTPS URL of the image to analyze when source=url"},"max_bytes":{"type":"integer","description":"Maximum local camera frame bytes when source=camera"},"question":{"type":"string","description":"A specific question about the image (default: describe the image in detail)"}}}"#
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_effect_class(ToolEffectClass::NetworkSearch)
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let m = parse_tool_args(args, STAGE)?;

        let question = m
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe this image in detail");
        if question.len() > 1024 {
            return Err(Error::config(STAGE, "question too long (max 1024)"));
        }

        match m.get("source").and_then(|v| v.as_str()).unwrap_or("url") {
            "url" => {
                let image_url = m
                    .get("image_url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config(STAGE, "missing or invalid image_url"))?;
                self.execute_url_vision(image_url, question, ctx)
            }
            "camera" => {
                let max_bytes = match m.get("max_bytes").and_then(|v| v.as_u64()) {
                    Some(value) => usize::try_from(value)
                        .map_err(|_| Error::config(STAGE, "max_bytes too large"))?,
                    None => MAX_CAMERA_CAPTURE_BYTES,
                };
                self.execute_local_camera(max_bytes, question, ctx)
            }
            _ => Err(Error::config(STAGE, "source must be url or camera")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{local_frame_fits_request_budget, AnalyzeImageTool, VisionImageInput};
    use crate::config::{AppConfig, LlmHeaderEntry, LlmModelKind, LlmSource};
    use crate::platform::{
        CameraFrameBuffer, CameraFrameFormat, CameraState, CameraStatus, PlatformCamera,
        ResponseBody,
    };
    use crate::runtime::mode::{snapshot_from_source, RuntimeModeSource};
    use crate::runtime::{
        FrameLeaseAdmission, RuntimeForegroundOverlay, RuntimeForegroundSource,
        RuntimePlanePolicyProfile, RuntimeSchedulerContext, RuntimeWorkSource,
    };
    use crate::tools::{Tool, ToolContext, ToolEffectClass};
    use base64::Engine as _;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct RecordingCamera {
        status: CameraStatus,
        captures: Arc<AtomicUsize>,
    }

    impl RecordingCamera {
        fn unavailable() -> (Arc<Self>, Arc<AtomicUsize>) {
            let captures = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    status: CameraStatus::unavailable("test_camera"),
                    captures: Arc::clone(&captures),
                }),
                captures,
            )
        }

        fn available() -> (Arc<Self>, Arc<AtomicUsize>) {
            let captures = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    status: CameraStatus {
                        state: CameraState::Available,
                        label: "test_camera",
                        max_frame_bytes: Some(1024),
                        formats: &[CameraFrameFormat::Jpeg],
                    },
                    captures: Arc::clone(&captures),
                }),
                captures,
            )
        }
    }

    impl PlatformCamera for RecordingCamera {
        fn camera_status(&self) -> CameraStatus {
            self.status
        }

        fn capture_frame(&self, max_bytes: usize) -> crate::Result<CameraFrameBuffer> {
            self.captures.fetch_add(1, Ordering::SeqCst);
            assert!(max_bytes >= 4);
            Ok(CameraFrameBuffer {
                bytes: crate::platform::ByteBuffer::from_vec(vec![0xff, 0xd8, 0xff, 0xd9]),
                format: CameraFrameFormat::Jpeg,
                width: 2,
                height: 2,
                captured_at_ms: 123,
                owner_label: "test_camera",
            })
        }
    }

    struct MockCtx {
        posts: usize,
        last_headers: Vec<(String, String)>,
        last_body: Vec<u8>,
    }

    impl ToolContext for MockCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> crate::Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> crate::Result<(u16, ResponseBody)> {
            self.posts += 1;
            self.last_headers = headers
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect();
            self.last_body = body.to_vec();
            Ok((
                200,
                ResponseBody::Heap(
                    br#"{"choices":[{"message":{"content":"vision text"}}]}"#.to_vec(),
                ),
            ))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    fn vision_config() -> AppConfig {
        let mut config = AppConfig::load_from_env();
        config.llm_sources = vec![LlmSource {
            id: "vision-source".to_string(),
            provider: "openai".to_string(),
            api_key: "test-key".to_string(),
            model: "gpt-vision-test".to_string(),
            api_url: "https://example.test/v1".to_string(),
            max_tokens: None,
            model_kind: LlmModelKind::Multimodal,
            custom_headers: Vec::<LlmHeaderEntry>::new(),
        }];
        config
    }

    fn normal_camera_admission() -> FrameLeaseAdmission {
        FrameLeaseAdmission::from_scheduler_context_for_tests(
            RuntimeWorkSource::UserFacing,
            RuntimeSchedulerContext {
                profile: RuntimePlanePolicyProfile::EspCompact,
                runtime_mode: snapshot_from_source(RuntimeModeSource::default()),
                foreground: RuntimeForegroundOverlay::default(),
                pressure: crate::orchestrator::PressureLevel::Normal,
            },
        )
    }

    fn foreground_background_camera_admission() -> FrameLeaseAdmission {
        FrameLeaseAdmission::from_scheduler_context_for_tests(
            RuntimeWorkSource::Background,
            RuntimeSchedulerContext {
                profile: RuntimePlanePolicyProfile::EspCompact,
                runtime_mode: snapshot_from_source(RuntimeModeSource::default()),
                foreground: RuntimeForegroundOverlay {
                    active: true,
                    active_count: 1,
                    primary_source: Some(RuntimeForegroundSource::ExternalUserMessage),
                    age_ms: Some(500),
                    resume_after_ms: Some(29_500),
                    ..RuntimeForegroundOverlay::default()
                },
                pressure: crate::orchestrator::PressureLevel::Normal,
            },
        )
    }

    #[test]
    fn analyze_image_metadata_marks_url_vision_as_network_search() {
        let tool = AnalyzeImageTool {
            sources: Vec::new(),
            camera: None,
            camera_admission_override: None,
        };

        assert_eq!(tool.metadata().effect_class, ToolEffectClass::NetworkSearch);
        assert!(tool.requires_network());
    }

    #[test]
    fn local_camera_capture_reports_unavailable_without_platform_capture() {
        let (camera, captures) = RecordingCamera::unavailable();
        let tool = AnalyzeImageTool::with_camera(&vision_config(), camera);
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };

        let err = tool
            .execute(
                r#"{"source":"camera","question":"what is visible?"}"#,
                &mut ctx,
            )
            .expect_err("unavailable camera must reject local capture");

        assert!(err.to_string().contains("camera_unavailable"));
        assert_eq!(captures.load(Ordering::SeqCst), 0);
        assert_eq!(ctx.posts, 0);
    }

    #[test]
    fn url_vision_does_not_consume_local_camera_capture() {
        let (camera, captures) = RecordingCamera::available();
        let tool = AnalyzeImageTool::with_camera(&vision_config(), camera);
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };

        let text = tool
            .execute(
                r#"{"image_url":"https://example.test/image.jpg","question":"describe"}"#,
                &mut ctx,
            )
            .expect("URL vision should keep existing network path");

        assert_eq!(text, "vision text");
        assert_eq!(captures.load(Ordering::SeqCst), 0);
        assert_eq!(ctx.posts, 1);
    }

    #[test]
    fn url_vision_requires_a_multimodal_source() {
        let mut config = vision_config();
        config.llm_sources[0].model = "text-only-model".to_string();
        config.llm_sources[0].model_kind = LlmModelKind::Text;
        let tool = AnalyzeImageTool::new(&config);
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };

        let text = tool
            .execute(
                r#"{"image_url":"https://example.test/image.jpg","question":"describe"}"#,
                &mut ctx,
            )
            .expect("missing multimodal source is a user-facing tool result");

        assert!(text.contains("no multimodal model configured"));
        assert_eq!(ctx.posts, 0);
    }

    #[test]
    fn url_vision_appends_custom_headers_without_overriding_builtin_headers() {
        let mut config = AppConfig::load_from_env();
        config.llm_sources = vec![serde_json::from_str(
            r#"{
                "id":"vision-source",
                "provider":"openai",
                "api_key":"test-key",
                "model":"gpt-vision-test",
                "api_url":"https://example.test/v1",
                "model_kind":"multimodal",
                "custom_headers":[
                    {"name":"X-Provider-Trace","value":"trace-1"},
                    {"name":"Authorization","value":"Bearer attacker"}
                ]
            }"#,
        )
        .expect("source json")];
        let tool = AnalyzeImageTool::new(&config);
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };

        let text = tool
            .execute(
                r#"{"image_url":"https://example.test/image.jpg","question":"describe"}"#,
                &mut ctx,
            )
            .expect("vision source should be used");

        let header_value = |name: &str| {
            ctx.last_headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(text, "vision text");
        assert_eq!(header_value("authorization"), Some("Bearer test-key"));
        assert_eq!(header_value("x-provider-trace"), Some("trace-1"));
    }

    #[test]
    fn local_camera_capture_rejects_oversize_before_platform_capture() {
        let (camera, captures) = RecordingCamera::available();
        let tool = AnalyzeImageTool::with_camera(&vision_config(), camera);
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };
        let args = format!(
            r#"{{"source":"camera","max_bytes":{},"question":"describe"}}"#,
            crate::runtime::frame_lease::MAX_CAMERA_CAPTURE_BYTES + 1
        );

        let err = tool
            .execute(&args, &mut ctx)
            .expect_err("oversize local camera capture must be rejected before capture");

        assert!(err.to_string().contains("camera_frame_max_bytes_exceeded"));
        assert_eq!(captures.load(Ordering::SeqCst), 0);
        assert_eq!(ctx.posts, 0);
    }

    #[test]
    fn local_camera_capture_respects_scheduler_defer_before_platform_capture() {
        let (camera, captures) = RecordingCamera::available();
        let tool = AnalyzeImageTool::with_camera_and_admission_for_tests(
            &vision_config(),
            camera,
            foreground_background_camera_admission(),
        );
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };

        let err = tool
            .execute(
                r#"{"source":"camera","max_bytes":1024,"question":"describe"}"#,
                &mut ctx,
            )
            .expect_err("scheduler defer must reject before platform capture");

        assert!(err.to_string().contains("foreground_active"));
        assert_eq!(captures.load(Ordering::SeqCst), 0);
        assert_eq!(ctx.posts, 0);
    }

    #[test]
    fn local_camera_capture_posts_real_frame_bytes_as_vision_input() {
        let (camera, captures) = RecordingCamera::available();
        let tool = AnalyzeImageTool::with_camera_and_admission_for_tests(
            &vision_config(),
            camera,
            normal_camera_admission(),
        );
        let mut ctx = MockCtx {
            posts: 0,
            last_headers: Vec::new(),
            last_body: Vec::new(),
        };

        let text = tool
            .execute(
                r#"{"source":"camera","max_bytes":1024,"question":"describe"}"#,
                &mut ctx,
            )
            .expect("available camera frame should be sent to vision source");

        assert_eq!(text, "vision text");
        assert_eq!(captures.load(Ordering::SeqCst), 1);
        assert_eq!(ctx.posts, 1);
        let body = String::from_utf8(ctx.last_body).expect("json body utf8");
        assert!(body.contains("data:image/jpeg;base64,/9j/2Q=="));
    }

    #[test]
    fn local_camera_openai_request_body_uses_external_preferred_buffer() {
        let frame = vec![0xa5; crate::platform::ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD];

        let body = AnalyzeImageTool::build_openai_request_body(
            "gpt-vision-test",
            VisionImageInput::LocalFrame {
                media_type: "image/jpeg",
                bytes: &frame,
            },
            "describe",
        )
        .expect("openai vision request body");

        assert!(body.is_external_preferred());
        let parsed: serde_json::Value = serde_json::from_slice(body.as_ref()).expect("json body");
        let url = parsed["messages"][0]["content"][0]["image_url"]["url"]
            .as_str()
            .expect("image url");
        let payload = url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("data url prefix");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("base64 payload");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn local_camera_anthropic_request_body_uses_external_preferred_buffer() {
        let frame = vec![0x5a; crate::platform::ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD];

        let body = AnalyzeImageTool::build_anthropic_request_body(
            "claude-vision-test",
            VisionImageInput::LocalFrame {
                media_type: "image/png",
                bytes: &frame,
            },
            "describe",
        )
        .expect("anthropic vision request body");

        assert!(body.is_external_preferred());
        let parsed: serde_json::Value = serde_json::from_slice(body.as_ref()).expect("json body");
        let source = &parsed["messages"][0]["content"][0]["source"];
        assert_eq!(source["type"], "base64");
        assert_eq!(source["media_type"], "image/png");
        let payload = source["data"].as_str().expect("base64 payload");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("base64 payload");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn local_camera_request_budget_accounts_for_base64_expansion() {
        assert!(!local_frame_fits_request_budget(
            crate::runtime::frame_lease::MAX_CAMERA_CAPTURE_BYTES,
            128,
            crate::runtime::frame_lease::MAX_CAMERA_CAPTURE_BYTES
        ));
        assert!(local_frame_fits_request_budget(1024, 128, 16 * 1024));
    }
}

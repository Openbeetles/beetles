//! 统一错误类型。所有公共 API 返回 `Result<T, Error>`，错误带上下文便于生产排查。
//! Unified error type. Public APIs return `Result<T, Error>` with context (stage, status_code).

use thiserror::Error;

/// 项目统一错误类型。各层通过 `?` 与 `.map_err()` 收敛；stage 为编译期字面量，避免错误路径堆分配。
/// Project-wide error. Use `?` and `.map_err()` with meaningful stage; no unwrap/expect in production paths.
#[derive(Error, Debug)]
pub enum Error {
    #[error("NVS (stage: {stage})")]
    Nvs {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        stage: &'static str,
    },

    #[error("storage (stage: {stage})")]
    Storage {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        stage: &'static str,
    },

    #[error("config: {message} (stage: {stage})")]
    Config {
        message: String,
        stage: &'static str,
    },

    #[error("IO: {source} (stage: {stage})")]
    Io {
        #[source]
        source: std::io::Error,
        stage: &'static str,
    },

    #[error("ESP-IDF: code={code} (stage: {stage})")]
    Esp { code: i32, stage: &'static str },

    #[error("HTTP: status={status_code} (stage: {stage})")]
    Http {
        status_code: u16,
        stage: &'static str,
    },

    #[error("other: {source} (stage: {stage})")]
    Other {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
        stage: &'static str,
    },
}

impl Error {
    fn nested_beetle_error(&self) -> Option<&Error> {
        match self {
            Error::Nvs {
                source: Some(source),
                ..
            }
            | Error::Storage {
                source: Some(source),
                ..
            }
            | Error::Other { source, .. } => source.downcast_ref::<Error>(),
            _ => None,
        }
    }

    pub fn nvs_stage(stage: &'static str) -> Self {
        Error::Nvs {
            source: None,
            stage,
        }
    }

    pub fn nvs(
        stage: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Error::Nvs {
            source: Some(Box::new(source)),
            stage,
        }
    }

    pub fn storage_stage(stage: &'static str) -> Self {
        Error::Storage {
            source: None,
            stage,
        }
    }

    pub fn storage(
        stage: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Error::Storage {
            source: Some(Box::new(source)),
            stage,
        }
    }

    pub fn config(stage: &'static str, message: impl Into<String>) -> Self {
        Error::Config {
            message: message.into(),
            stage,
        }
    }

    pub fn io(stage: &'static str, source: std::io::Error) -> Self {
        Error::Io { source, stage }
    }

    pub fn esp(stage: &'static str, code: i32) -> Self {
        Error::Esp { code, stage }
    }

    pub fn http(stage: &'static str, status_code: u16) -> Self {
        Error::Http { status_code, stage }
    }

    /// 返回错误的 stage 字符串，便于日志与监控。
    pub fn stage(&self) -> &'static str {
        match self {
            Error::Nvs { stage, .. } => stage,
            Error::Storage { stage, .. } => stage,
            Error::Config { stage, .. } => stage,
            Error::Io { stage, .. } => stage,
            Error::Esp { stage, .. } => stage,
            Error::Http { stage, .. } => stage,
            Error::Other { stage, .. } => stage,
        }
    }

    /// 是否为 TLS 准入相关错误（供退避/重试判定）。
    pub fn is_tls_admission(&self) -> bool {
        self.stage() == "tls_admission"
            || self
                .nested_beetle_error()
                .is_some_and(|source| source.is_tls_admission())
    }

    /// 是否为连接层失败（TLS 握手超时、socket 连接失败等）。
    /// 此类错误短时间重试大概率仍会失败，应快速失败而非级联阻塞。
    pub fn is_connect_error(&self) -> bool {
        let local = match self {
            Error::Io { stage, .. } | Error::Other { stage, .. } => matches!(
                *stage,
                "http_post_request"
                    | "http_get_request"
                    | "http_post_submit"
                    | "http_get_submit"
                    | "http_client_replace"
                    | "http_read"
            ),
            _ => false,
        };
        local
            || self
                .nested_beetle_error()
                .is_some_and(|source| source.is_connect_error())
    }

    pub fn http_status_code(&self) -> Option<u16> {
        match self {
            Error::Http { status_code, .. } => Some(*status_code),
            _ => None,
        }
    }

    pub fn is_retryable_upstream(&self) -> bool {
        if self.is_tls_admission() || self.is_connect_error() {
            return true;
        }
        matches!(
            self.http_status_code(),
            Some(408 | 409 | 425 | 429) | Some(500..=599)
        )
    }

    pub fn metrics_stage(&self) -> &'static str {
        if self.stage() == "tls_admission" || self.is_connect_error() {
            return self.stage();
        }
        self.nested_beetle_error()
            .map(|source| source.metrics_stage())
            .unwrap_or_else(|| self.stage())
    }

    /// 覆盖 stage 并返回同一变体，便于保留 Config/Http 等判别用于监控与排查。
    pub fn with_stage(self, stage: &'static str) -> Self {
        match self {
            Error::Nvs { source, .. } => Error::Nvs { source, stage },
            Error::Storage { source, .. } => Error::Storage { source, stage },
            Error::Config { message, .. } => Error::Config { message, stage },
            Error::Io { source, .. } => Error::Io { source, stage },
            Error::Esp { code, .. } => Error::Esp { code, stage },
            Error::Http { status_code, .. } => Error::Http { status_code, stage },
            Error::Other { source, .. } => Error::Other { source, stage },
        }
    }
}

/// 简化 `Result<T, Error>` 类型别名。
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn http_status_is_not_classified_as_connect_error() {
        let err = Error::Http {
            status_code: 400,
            stage: "http_post_request",
        };
        assert!(!err.is_connect_error());
        assert_eq!(err.http_status_code(), Some(400));
        assert!(!err.is_retryable_upstream());
    }

    #[test]
    fn retryable_http_statuses_are_marked_retryable() {
        let err = Error::Http {
            status_code: 429,
            stage: "llm_http_status",
        };
        assert!(err.is_retryable_upstream());
    }

    #[test]
    fn wrapped_tls_admission_error_stays_retryable() {
        let err = Error::Other {
            source: Box::new(Error::config("tls_admission", "largest block too small")),
            stage: "llm_request",
        };
        assert!(err.is_tls_admission());
        assert!(err.is_retryable_upstream());
    }

    #[test]
    fn http_submit_stages_are_retryable_connect_errors() {
        for stage in ["http_post_submit", "http_get_submit"] {
            let err = Error::Io {
                source: std::io::Error::other("socket submit failed"),
                stage,
            };
            assert!(err.is_connect_error(), "{stage} should be a connect error");
            assert!(err.is_retryable_upstream(), "{stage} should be retryable");
        }
    }
}

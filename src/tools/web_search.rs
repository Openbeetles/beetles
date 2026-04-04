//! web_search 工具：Tavily 优先、Brave 兜底；返回结构化结果，便于后续 web_fetch。
//! web_search tool: Tavily first, Brave fallback; returns structured results for follow-up fetch.

use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::tools::{Tool, ToolContext, parse_tool_args};
use crate::util::percent_encode_query;
use serde::Serialize;
use serde_json::{Value, json};

const TAG: &str = "tools::web_search";
const BRAVE_SEARCH_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const TAVILY_SEARCH_URL: &str = "https://api.tavily.com/search";
const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SearchResultItem {
    title: String,
    url: String,
    snippet: String,
}

pub struct WebSearchTool {
    pub api_key: String,
    pub tavily_key: String,
}

impl WebSearchTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            api_key: config.search_key.clone(),
            tavily_key: config.tavily_key.clone(),
        }
    }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn description(&self) -> &'static str {
        "Search the web and return structured results with titles, URLs, and snippets. Use document_read with one of the returned URLs when page or document content is needed."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Maximum results to return (default 5, max 8)"}},"required":["query"]}"#
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let m = parse_tool_args(args, "tool_web_search")?;
        let query = m
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_web_search", "missing or invalid query"))?;
        let limit = parse_limit(m.get("limit"));

        if !self.tavily_key.is_empty() {
            let body = json!({
                "query": query,
                "max_results": limit,
            });
            let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "tool_web_search",
            })?;
            let auth = format!("Bearer {}", self.tavily_key);
            let headers: [(&str, &str); 2] = [
                ("Content-Type", "application/json"),
                ("Authorization", &auth),
            ];
            let (status, resp_body) =
                match ctx.post_with_headers(TAVILY_SEARCH_URL, &headers, &body_bytes) {
                    Ok(r) => r,
                    Err(e) => {
                        log::warn!("[{}] Tavily request failed: {:?}", TAG, e);
                        return self.fallback_brave_or_warning(query, limit, ctx);
                    }
                };
            if (200..300).contains(&status) {
                let parsed = serde_json::from_slice::<serde_json::Value>(resp_body.as_ref())
                    .map_err(|e| Error::Other {
                        source: Box::new(e),
                        stage: "tool_web_search",
                    })?;
                let results = parse_tavily_results(&parsed, limit);
                log::info!(
                    "[{}] Tavily query len={} results={}",
                    TAG,
                    query.len(),
                    results.len()
                );
                return build_search_response(query, "tavily", &results, None);
            }
            log::warn!("[{}] Tavily status={}, fallback to Brave", TAG, status);
            return self.fallback_brave_or_warning(query, limit, ctx);
        }

        if !self.api_key.is_empty() {
            return self.do_brave(query, limit, ctx);
        }

        build_search_response(
            query,
            "none",
            &[],
            Some("web_search: no search provider configured"),
        )
    }
}

impl WebSearchTool {
    fn fallback_brave_or_warning(
        &self,
        query: &str,
        limit: usize,
        ctx: &mut dyn ToolContext,
    ) -> Result<String> {
        if self.api_key.is_empty() {
            return build_search_response(
                query,
                "tavily",
                &[],
                Some("web_search: Tavily request failed and no Brave key is configured"),
            );
        }
        self.do_brave(query, limit, ctx)
    }

    fn do_brave(&self, query: &str, limit: usize, ctx: &mut dyn ToolContext) -> Result<String> {
        let url = format!(
            "{}?q={}&count={}",
            BRAVE_SEARCH_URL,
            percent_encode_query(query),
            limit
        );
        let headers = [("X-Subscription-Token", self.api_key.as_str())];
        let (status, body) = ctx.get_with_headers(&url, &headers).map_err(|e| match e {
            Error::Http { status_code, .. } => Error::Http {
                status_code,
                stage: "tool_web_search",
            },
            _ => Error::Other {
                source: Box::new(std::io::Error::other(format!("{:?}", e))),
                stage: "tool_web_search",
            },
        })?;
        if status >= 400 {
            return Err(Error::Http {
                status_code: status,
                stage: "tool_web_search",
            });
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(body.as_ref()).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "tool_web_search",
            })?;
        let results = parse_brave_results(&parsed, limit);
        log::info!(
            "[{}] Brave query len={} results={}",
            TAG,
            query.len(),
            results.len()
        );
        build_search_response(query, "brave", &results, None)
    }
}

fn parse_limit(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|raw| raw.clamp(1, MAX_LIMIT as u64) as usize)
        .unwrap_or(DEFAULT_LIMIT)
}

fn parse_tavily_results(value: &Value, limit: usize) -> Vec<SearchResultItem> {
    value
        .get("results")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(limit)
                .filter_map(|item| {
                    let url = item.get("url").and_then(Value::as_str)?.trim();
                    if url.is_empty() {
                        return None;
                    }
                    Some(SearchResultItem {
                        title: item
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        url: url.to_string(),
                        snippet: item
                            .get("content")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_brave_results(value: &Value, limit: usize) -> Vec<SearchResultItem> {
    value
        .get("web")
        .and_then(|v| v.get("results"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(limit)
                .filter_map(|item| {
                    let url = item.get("url").and_then(Value::as_str)?.trim();
                    if url.is_empty() {
                        return None;
                    }
                    Some(SearchResultItem {
                        title: item
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        url: url.to_string(),
                        snippet: item
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn build_summary(results: &[SearchResultItem]) -> String {
    let snippets: Vec<&str> = results
        .iter()
        .filter_map(|item| {
            let snippet = item.snippet.trim();
            (!snippet.is_empty()).then_some(snippet)
        })
        .take(3)
        .collect();
    if snippets.is_empty() {
        "no results".to_string()
    } else {
        snippets.join(" ")
    }
}

fn build_search_response(
    query: &str,
    provider: &str,
    results: &[SearchResultItem],
    warning: Option<&str>,
) -> Result<String> {
    serde_json::to_string(&json!({
        "query": query,
        "provider": provider,
        "count": results.len(),
        "results": results,
        "summary": build_summary(results),
        "warning": warning,
    }))
    .map_err(|e| Error::config("tool_web_search", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{SearchResultItem, build_summary, parse_brave_results, parse_tavily_results};
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::ResponseBody;
    use crate::tools::{Tool, ToolContext, WebSearchTool};
    use serde_json::{Value, json};

    struct MockToolContext {
        post_status: u16,
        post_body: Value,
        get_status: u16,
        get_body: Value,
    }

    impl ToolContext for MockToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Ok((
                self.get_status,
                ResponseBody::Heap(self.get_body.to_string().into_bytes()),
            ))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Ok((
                self.post_status,
                ResponseBody::Heap(self.post_body.to_string().into_bytes()),
            ))
        }

        fn current_chat_id(&self) -> Option<&str> {
            None
        }

        fn current_channel(&self) -> Option<&str> {
            None
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    #[test]
    fn parses_tavily_results_into_structured_items() {
        let parsed = parse_tavily_results(
            &json!({
                "results": [
                    {"title": "A", "url": "https://a.example", "content": "alpha"},
                    {"title": "B", "url": "https://b.example", "content": "beta"}
                ]
            }),
            5,
        );
        assert_eq!(
            parsed,
            vec![
                SearchResultItem {
                    title: "A".into(),
                    url: "https://a.example".into(),
                    snippet: "alpha".into(),
                },
                SearchResultItem {
                    title: "B".into(),
                    url: "https://b.example".into(),
                    snippet: "beta".into(),
                }
            ]
        );
    }

    #[test]
    fn parses_brave_results_into_structured_items() {
        let parsed = parse_brave_results(
            &json!({
                "web": {
                    "results": [
                        {"title": "A", "url": "https://a.example", "description": "alpha"}
                    ]
                }
            }),
            5,
        );
        assert_eq!(
            parsed,
            vec![SearchResultItem {
                title: "A".into(),
                url: "https://a.example".into(),
                snippet: "alpha".into(),
            }]
        );
    }

    #[test]
    fn summary_prefers_top_non_empty_snippets() {
        let summary = build_summary(&[
            SearchResultItem {
                title: "A".into(),
                url: "https://a.example".into(),
                snippet: "".into(),
            },
            SearchResultItem {
                title: "B".into(),
                url: "https://b.example".into(),
                snippet: "beta".into(),
            },
        ]);
        assert_eq!(summary, "beta");
    }

    #[test]
    fn execute_returns_structured_results_json() {
        let tool = WebSearchTool {
            api_key: String::new(),
            tavily_key: "tavily-key".into(),
        };
        let mut ctx = MockToolContext {
            post_status: 200,
            post_body: json!({
                "results": [
                    {"title": "Example", "url": "https://example.com", "content": "hello world"}
                ]
            }),
            get_status: 200,
            get_body: json!({}),
        };

        let result = tool
            .execute(r#"{"query":"example","limit":1}"#, &mut ctx)
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["provider"], "tavily");
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["results"][0]["url"], "https://example.com");
        assert_eq!(parsed["summary"], "hello world");
    }
}

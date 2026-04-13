//! Programmable tool-request proposal contracts for capability-scoped bridge expansion.

use crate::error::{Error, Result};
use crate::tools::MAX_TOOL_ARGS_LEN;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TOOL_REQUEST_MAX_SUMMARY_CHARS: usize = 220;
const TOOL_REQUEST_MAX_PROPOSALS: usize = 6;
const TOOL_REQUEST_MAX_TOOL_NAME_CHARS: usize = 64;
const TOOL_REQUEST_MAX_PROPOSAL_SUMMARY_CHARS: usize = 160;
const TOOL_REQUEST_MAX_PROPOSAL_RATIONALE_CHARS: usize = 220;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRequestProposal {
    pub tool_name: String,
    pub summary: String,
    pub rationale: String,
    pub args: Value,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRequestResult {
    pub summary: String,
    #[serde(default)]
    pub tool_request_proposals: Vec<ToolRequestProposal>,
}

pub fn validate_tool_request_result(value: Value) -> Result<ToolRequestResult> {
    let result: ToolRequestResult = serde_json::from_value(value)
        .map_err(|error| Error::config("tool_request_result_decode", error.to_string()))?;
    normalize_tool_request_result(result)
}

fn normalize_tool_request_result(result: ToolRequestResult) -> Result<ToolRequestResult> {
    let summary =
        truncate_content_to_max(result.summary.trim(), TOOL_REQUEST_MAX_SUMMARY_CHARS).into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "tool_request_result_validate",
            "missing summary",
        ));
    }
    if result.tool_request_proposals.len() > TOOL_REQUEST_MAX_PROPOSALS {
        return Err(Error::config(
            "tool_request_result_validate",
            format!(
                "tool_request_proposals exceeds {}",
                TOOL_REQUEST_MAX_PROPOSALS
            ),
        ));
    }
    let tool_request_proposals = result
        .tool_request_proposals
        .into_iter()
        .map(normalize_tool_request_proposal)
        .collect::<Result<Vec<_>>>()?;
    Ok(ToolRequestResult {
        summary,
        tool_request_proposals,
    })
}

fn normalize_tool_request_proposal(proposal: ToolRequestProposal) -> Result<ToolRequestProposal> {
    let tool_name =
        truncate_content_to_max(proposal.tool_name.trim(), TOOL_REQUEST_MAX_TOOL_NAME_CHARS)
            .into_owned();
    let summary = truncate_content_to_max(
        proposal.summary.trim(),
        TOOL_REQUEST_MAX_PROPOSAL_SUMMARY_CHARS,
    )
    .into_owned();
    let rationale = truncate_content_to_max(
        proposal.rationale.trim(),
        TOOL_REQUEST_MAX_PROPOSAL_RATIONALE_CHARS,
    )
    .into_owned();
    if tool_name.is_empty() || summary.is_empty() || rationale.is_empty() {
        return Err(Error::config(
            "tool_request_proposal_validate",
            "tool request proposal requires tool_name, summary, and rationale",
        ));
    }
    if !proposal.requires_adjudication {
        return Err(Error::config(
            "tool_request_proposal_validate",
            "tool request proposal must require adjudication",
        ));
    }
    if !proposal.args.is_object() {
        return Err(Error::config(
            "tool_request_proposal_validate",
            "tool request proposal args must be a JSON object",
        ));
    }
    let arg_bytes = serde_json::to_vec(&proposal.args)
        .map_err(|error| Error::config("tool_request_proposal_validate", error.to_string()))?;
    if arg_bytes.len() > MAX_TOOL_ARGS_LEN {
        return Err(Error::config(
            "tool_request_proposal_validate",
            format!("tool request args length exceeds {}", MAX_TOOL_ARGS_LEN),
        ));
    }
    Ok(ToolRequestProposal {
        tool_name,
        summary,
        rationale,
        args: proposal.args,
        requires_adjudication: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_tool_request_result_accepts_adjudicated_proposals() {
        let result = validate_tool_request_result(json!({
            "summary": "Prepared one tool request proposal.",
            "tool_request_proposals": [{
                "tool_name": "files",
                "summary": "Inspect the current workspace tree.",
                "rationale": "A directory listing is needed before planning edits.",
                "args": {"path": ".", "mode": "list"},
                "requires_adjudication": true
            }]
        }))
        .expect("valid tool request proposal result");

        assert_eq!(result.tool_request_proposals.len(), 1);
        assert_eq!(result.tool_request_proposals[0].tool_name, "files");
    }

    #[test]
    fn validate_tool_request_result_rejects_non_adjudicated_proposal() {
        let error = validate_tool_request_result(json!({
            "summary": "bad",
            "tool_request_proposals": [{
                "tool_name": "files",
                "summary": "Inspect the current workspace tree.",
                "rationale": "A directory listing is needed before planning edits.",
                "args": {"path": ".", "mode": "list"},
                "requires_adjudication": false
            }]
        }))
        .expect_err("proposal should be rejected");

        assert!(error
            .to_string()
            .contains("tool request proposal must require adjudication"));
    }
}

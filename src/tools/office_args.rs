use crate::error::{Error, Result};
use crate::office::OfficeAccountIdentityClass;
use serde_json::{Map, Value};

pub(crate) fn parse_preferred_identity_class(
    obj: &Map<String, Value>,
    field: &str,
    stage: &'static str,
) -> Result<Option<OfficeAccountIdentityClass>> {
    obj.get(field)
        .map(|value| parse_identity_class_value(value, field, stage))
        .transpose()
}

pub(crate) fn parse_identity_class_value(
    value: &Value,
    field: &str,
    stage: &'static str,
) -> Result<OfficeAccountIdentityClass> {
    let raw = value
        .as_str()
        .ok_or_else(|| Error::config(stage, format!("{field} must be a string")))?;
    match raw {
        "work" => Ok(OfficeAccountIdentityClass::Work),
        "personal" => Ok(OfficeAccountIdentityClass::Personal),
        "family" => Ok(OfficeAccountIdentityClass::Family),
        "shared" => Ok(OfficeAccountIdentityClass::Shared),
        "other" => Ok(OfficeAccountIdentityClass::Other),
        _ => Err(Error::config(stage, format!("unsupported {field} '{raw}'"))),
    }
}

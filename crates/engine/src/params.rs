use disccraft_project::{DiscSettings, Link, Tick, parse_time};
use serde_json::Value;

/// Returns a required unsigned integer parameter.
pub(crate) fn req_u64(params: &Value, name: &str) -> Result<u64, String> {
    let value = get(params, name)?;
    as_u64(value, name)
}

/// Returns an optional unsigned integer parameter.
#[allow(dead_code)]
pub(crate) fn opt_u64(params: &Value, name: &str) -> Result<Option<u64>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(as_u64(value, name)?)),
    }
}

/// Returns a required string parameter.
pub(crate) fn req_str(params: &Value, name: &str) -> Result<String, String> {
    let value = get(params, name)?;
    value
        .as_str()
        .ok_or_else(|| format!("parameter \"{name}\" must be a string"))
        .map(str::to_string)
}

/// Returns an optional string parameter.
pub(crate) fn opt_str(params: &Value, name: &str) -> Result<Option<String>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .ok_or_else(|| format!("parameter \"{name}\" must be a string"))
            .map(|text| Some(text.to_string())),
    }
}

/// Returns a required index parameter.
pub(crate) fn req_usize(params: &Value, name: &str) -> Result<usize, String> {
    let value = req_u64(params, name)?;
    usize::try_from(value).map_err(|_| format!("parameter \"{name}\" must be a valid index"))
}

/// Returns a required link parameter.
pub(crate) fn req_link(params: &Value, name: &str) -> Result<Link, String> {
    let value = get(params, name)?;
    serde_json::from_value::<Link>(value.clone())
        .map_err(|error| format!("parameter \"{name}\" must be a valid link: {error}"))
}

/// Returns a required JSON value parameter.
pub(crate) fn req_value<'a>(params: &'a Value, name: &str) -> Result<&'a Value, String> {
    get(params, name)
}

/// Returns an optional JSON value parameter.
pub(crate) fn opt_value<'a>(params: &'a Value, name: &str) -> Option<&'a Value> {
    params.get(name).filter(|value| !value.is_null())
}

/// Parses either a timecode or raw tick time parameter.
pub(crate) fn time_param(params: &Value, settings: &DiscSettings) -> Result<Tick, String> {
    if let Some(value) = params.get("timecode")
        && !value.is_null()
    {
        let text = value
            .as_str()
            .ok_or_else(|| "parameter \"timecode\" must be a string".to_string())?;
        return parse_time(text, settings)
            .map_err(|error| format!("parameter \"timecode\" is invalid: {error}"));
    }

    if let Some(value) = params.get("time")
        && !value.is_null()
    {
        let ticks = value
            .as_i64()
            .ok_or_else(|| "parameter \"time\" must be an integer".to_string())?;
        return Ok(Tick(ticks));
    }

    Err("missing parameter \"timecode\" or \"time\"".to_string())
}

fn get<'a>(params: &'a Value, name: &str) -> Result<&'a Value, String> {
    params
        .get(name)
        .ok_or_else(|| format!("missing parameter \"{name}\""))
}

fn as_u64(value: &Value, name: &str) -> Result<u64, String> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| format!("parameter \"{name}\" must be a non-negative integer")),
        _ => Err(format!(
            "parameter \"{name}\" must be a non-negative integer"
        )),
    }
}

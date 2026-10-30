//! Native CC Switch modelCatalog merge by model identity. Only fields created
//! by this adapter are managed; names, notes, top-level model choice and other
//! provider settings stay with the user. Prices are an informational snapshot.
use super::{BindingData, BindingError, Result};
use crate::oauth::applications::VerifiedCatalog;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn source(proof: &VerifiedCatalog, group: &str) -> Result<(Value, Value)> {
    if group.is_empty() || group.len() > 64 || group.chars().any(char::is_control) {
        return Err(BindingError::Invalid);
    }
    let group_id = URL_SAFE_NO_PAD.encode(group.as_bytes());
    let mut names = BTreeSet::new();
    let mut native = Vec::new();
    let mut prices = Vec::new();
    for model in proof.catalog().models.iter().filter(|model| {
        model.group == group && model.apis.iter().any(|api| api == "openai-responses")
    }) {
        if model.upstream_model.is_empty()
            || model.upstream_model.len() > 1024
            || model.name.len() > 1024
            || model.name.chars().any(char::is_control)
            || model.upstream_model.chars().any(char::is_control)
            || model.id
                != format!(
                    "lmm:{group_id}:{}",
                    URL_SAFE_NO_PAD.encode(model.upstream_model.as_bytes())
                )
            || !names.insert(model.upstream_model.clone())
        {
            return Err(BindingError::Invalid);
        }
        native.push(json!({"model":model.upstream_model,"displayName":model.name,"inputModalities":["text"]}));
        prices.push(json!({"id":model.id,"model":model.upstream_model,"pricing":model.pricing}));
    }
    if native.len() > 4096 {
        return Err(BindingError::Invalid);
    }
    native.sort_by(|left, right| left["model"].as_str().cmp(&right["model"].as_str()));
    prices.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    Ok((json!({"models":native}), Value::Array(prices)))
}

fn rows(value: &Value) -> Result<BTreeMap<String, Value>> {
    let rows = value
        .as_object()
        .and_then(|object| object.get("models"))
        .and_then(Value::as_array)
        .ok_or(BindingError::Conflict)?;
    if rows.len() > 4096 {
        return Err(BindingError::Conflict);
    }
    let mut result = BTreeMap::new();
    for row in rows {
        let id = row
            .as_object()
            .and_then(|row| row.get("model"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or(BindingError::Conflict)?;
        if result.insert(id.to_owned(), row.clone()).is_some() {
            return Err(BindingError::Conflict);
        }
    }
    Ok(result)
}

/// A user override of an existing scalar wins. A new item cannot capture the
/// identity of a pre-existing user item. Removing an edited managed item is a
/// conflict, because a broad delete would erase those subsequent edits.
pub(super) fn merge(current: &Value, before: &Value, after: &Value) -> Result<Value> {
    let previous = rows(before)?;
    let desired = rows(after)?;
    let mut merged = rows(current)?;
    for (id, old) in &previous {
        let Some(actual) = merged.get(id) else {
            continue;
        }; // Preserve a user's deletion.
        match desired.get(id) {
            None => {
                if actual != old {
                    return Err(BindingError::Conflict);
                }
                merged.remove(id);
            }
            Some(new) if old != new => {
                let mut next = actual.as_object().cloned().ok_or(BindingError::Conflict)?;
                let old = old.as_object().ok_or(BindingError::Conflict)?;
                let new = new.as_object().ok_or(BindingError::Conflict)?;
                for key in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
                    if old.get(key) == new.get(key) {
                        continue;
                    }
                    if next.get(key) == old.get(key) || next.get(key) == new.get(key) {
                        match new.get(key) {
                            Some(value) => {
                                next.insert(key.clone(), value.clone());
                            }
                            None => {
                                next.remove(key);
                            }
                        }
                    }
                }
                merged.insert(id.clone(), Value::Object(next));
            }
            Some(_) => {}
        }
    }
    for (id, next) in &desired {
        if previous.contains_key(id) {
            continue;
        }
        if merged.contains_key(id) {
            return Err(BindingError::Conflict);
        }
        merged.insert(id.clone(), next.clone());
    }
    let mut result = current.as_object().cloned().ok_or(BindingError::Conflict)?;
    // Retain the user's order for existing items; append new managed models in
    // deterministic order. Model identity, rather than array index, is the CAS.
    let mut output = Vec::new();
    for row in current["models"].as_array().ok_or(BindingError::Conflict)? {
        if let Some(id) = row["model"].as_str()
            && let Some(value) = merged.remove(id)
        {
            output.push(value);
        }
    }
    output.extend(merged.into_values());
    result.insert("models".into(), Value::Array(output));
    Ok(Value::Object(result))
}

fn quoted(value: &str) -> Result<String> {
    // JSON basic string escapes are a valid subset of TOML basic string escapes
    // for the UTF-8, non-control text accepted at these inputs.
    serde_json::to_string(value).map_err(|_| BindingError::Invalid)
}
pub(super) fn config(binding: &BindingData) -> Result<String> {
    let args = [
        "credential",
        "cc-switch",
        "--issuer",
        &binding.issuer,
        "--installation-id",
        &binding.installation_id,
        "--grant-id",
        &binding.grant_id,
    ];
    let args = args
        .iter()
        .map(|value| quoted(value))
        .collect::<Result<Vec<_>>>()?
        .join(", ");
    Ok(format!(
        concat!(
            "model_provider = \"lmm\"\nmodel = {}\ndisable_response_storage = true\nweb_search = \"disabled\"\n\n",
            "[model_providers.lmm]\nname = \"LMM\"\nbase_url = {}\nwire_api = \"responses\"\n",
            "requires_openai_auth = false\nsupports_websockets = false\n\n",
            "[model_providers.lmm.http_headers]\nx-lmm-group = {}\n\n",
            "[model_providers.lmm.auth]\ncommand = {}\nargs = [{}]\ntimeout_ms = 45000\n",
            "refresh_interval_ms = 60000\ncwd = {}\n"
        ),
        quoted(&binding.initial_model)?,
        quoted(&format!("{}/v1", binding.issuer))?,
        quoted(&URL_SAFE_NO_PAD.encode(binding.group.as_bytes()))?,
        quoted(&binding.helper_path)?,
        args,
        quoted(&binding.helper_directory)?
    ))
}

/// CC Switch may normalize model_provider=lmm to model_provider=custom. Compare
/// the selected native provider table semantically, while preserving the exact
/// user's TOML text and all unrelated top-level settings.
pub(super) fn selected_model(settings: &Value, binding: &BindingData) -> Result<String> {
    let actual: toml::Value =
        toml::from_str(settings["config"].as_str().ok_or(BindingError::Conflict)?)
            .map_err(|_| BindingError::Conflict)?;
    let expected: toml::Value =
        toml::from_str(&config(binding)?).map_err(|_| BindingError::Invalid)?;
    let provider = actual
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .ok_or(BindingError::Conflict)?;
    let table = actual
        .get("model_providers")
        .and_then(|providers| providers.get(provider));
    if table
        != expected
            .get("model_providers")
            .and_then(|providers| providers.get("lmm"))
    {
        return Err(BindingError::Conflict);
    }
    actual
        .get("model")
        .and_then(toml::Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
        .ok_or(BindingError::Conflict)
}
pub(super) fn contains(catalog: &Value, model: &str) -> Result<bool> {
    Ok(rows(catalog)?.contains_key(model))
}

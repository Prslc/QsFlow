use serde_json::{json, Value};
use tokio::sync::mpsc;

const INVALID_REQUEST: (i64, &str) = (-32600, "Invalid Request");
const METHOD_NOT_FOUND: (i64, &str) = (-32601, "Method not found");
const INVALID_PARAMS: (i64, &str) = (-32602, "Invalid params");

async fn respond(tx: &mpsc::Sender<String>, id: Value, result: Result<Value, (i64, &str)>) {
    let payload = match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "result": result, "id": id }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "error": { "code": code, "message": message },
            "id": id,
        }),
    };
    crate::emit(tx, &payload).await;
}

fn search_text(params: &Option<Value>) -> Result<String, ()> {
    match params {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::Object(map)) => match map.get("text") {
            Some(Value::String(s)) => Ok(s.clone()),
            _ => Err(()), // text missing or not a string
        },
        _ => Err(()),
    }
}

fn select_payload(params: &Option<Value>) -> Result<String, ()> {
    match params {
        Some(obj @ Value::Object(_)) => Ok(obj.to_string()),
        _ => Err(()),
    }
}

fn forget_key(params: &Option<Value>) -> Result<String, ()> {
    match params {
        Some(Value::Object(map)) => map
            .get("on_click")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or(()),
        _ => Err(()),
    }
}

fn run_cmd(params: &Option<Value>) -> Result<String, ()> {
    match params {
        Some(Value::Object(map)) => map
            .get("cmd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or(()),
        _ => Err(()),
    }
}

/// Handle a line as a JSON-RPC 2.0 request. Returns `true` if `line` was a
/// JSON-RPC request (valid or not); `false` if it should fall through to the
/// legacy text protocol.
pub async fn handle(line: &str, tx: &mpsc::Sender<String>) -> bool {
    // JSON-RPC requests are always objects; skip the JSON parse on the hot
    // text-search path where most lines are plain queries.
    if !line.starts_with('{') {
        return false;
    }

    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return false,
    };

    if value.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        return false;
    }

    let has_id = value.get("id").is_some();
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value.get("method").and_then(|v| v.as_str());
    let params = value.get("params").cloned();

    let Some(method) = method else {
        if has_id {
            respond(tx, id, Err(INVALID_REQUEST)).await;
        }
        return true;
    };

    match method {
        "search" => {
            let text = match search_text(&params) {
                Ok(t) => t,
                Err(()) => {
                    if has_id {
                        respond(tx, id, Err(INVALID_PARAMS)).await;
                    }
                    return true;
                }
            };
            // Await inline (request/response), unlike the streaming text-search
            // path: a JSON-RPC client gets its correlated response even for a
            // one-shot `printf ... | qsflow-core` (no need to hold stdin open).
            if text.is_empty() {
                // `top` is the dedicated most-used method; an empty `search`
                // query is not a search. (The text protocol handles its own
                // empty-line default in main.rs, so this never affects the UI.)
                if has_id {
                    respond(tx, id, Err(INVALID_PARAMS)).await;
                }
                return true;
            }
            if has_id {
                let results = crate::plugin::dispatch(&text).await;
                respond(tx, id, Ok(json!(results))).await;
            }
        }
        "top" => {
            let items = crate::system::usage::get_top(20).unwrap_or_default();
            if has_id {
                respond(tx, id, Ok(json!(items))).await;
            }
        }
        "select" => {
            let payload = match select_payload(&params) {
                Ok(p) => p,
                Err(()) => {
                    if has_id {
                        respond(tx, id, Err(INVALID_PARAMS)).await;
                    }
                    return true;
                }
            };
            let _ = crate::system::usage::record(&payload);
            if has_id {
                respond(tx, id, Ok(Value::Null)).await;
            }
        }
        "forget" => {
            let key = match forget_key(&params) {
                Ok(k) => k,
                Err(()) => {
                    if has_id {
                        respond(tx, id, Err(INVALID_PARAMS)).await;
                    }
                    return true;
                }
            };
            let _ = crate::system::usage::forget(&key);
            crate::plugin::forget_row(&key).await;
            if has_id {
                respond(tx, id, Ok(Value::Null)).await;
            }
        }
        "run" => {
            let cmd = match run_cmd(&params) {
                Ok(c) => c,
                Err(()) => {
                    if has_id {
                        respond(tx, id, Err(INVALID_PARAMS)).await;
                    }
                    return true;
                }
            };
            crate::system::executor::execute_command(&cmd);
            if has_id {
                respond(tx, id, Ok(Value::Null)).await;
            }
        }
        "resolve_icon" => {
            // Resolve any icon spec (absolute path, theme name, or the
            // `papirus:<name>` scheme) to the absolute path the UI renders.
            let name = match params {
                Some(Value::Object(map)) => match map.get("name") {
                    Some(Value::String(s)) if !s.is_empty() => s.clone(),
                    _ => {
                        if has_id {
                            respond(tx, id, Err(INVALID_PARAMS)).await;
                        }
                        return true;
                    }
                },
                _ => {
                    if has_id {
                        respond(tx, id, Err(INVALID_PARAMS)).await;
                    }
                    return true;
                }
            };
            if has_id {
                respond(tx, id, Ok(json!(crate::system::icon::find_icon_path(&name)))).await;
            }
        }
        "list_plugins" => {
            let plugins: Vec<Value> = crate::plugin::list_plugins().await
                .into_iter()
                .map(|(pid, name, icon, keyword, enabled)| {
                    json!({
                        "id": pid,
                        "name": name,
                        "icon": icon,
                        "keyword": keyword,
                        "enabled": enabled,
                    })
                })
                .collect();
            if has_id {
                respond(tx, id, Ok(json!(plugins))).await;
            }
        }
        "theme" => {
            let theme = crate::system::theme::load_theme();
            let value = serde_json::to_value(&theme).unwrap_or(Value::Null);
            if has_id {
                respond(tx, id, Ok(value)).await;
            }
        }
        "ping" => {
            if has_id {
                respond(tx, id, Ok(json!("pong"))).await;
            }
        }
        _ => {
            if has_id {
                respond(tx, id, Err(METHOD_NOT_FOUND)).await;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    async fn run(line: &str) -> (bool, Vec<String>) {
        let (tx, mut rx) = mpsc::channel::<String>(32);
        let handled = handle(line, &tx).await;
        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() {
            msgs.push(m);
        }
        (handled, msgs)
    }

    #[tokio::test]
    async fn ping_returns_pong_with_id() {
        let (handled, msgs) = run(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#).await;
        assert!(handled);
        assert_eq!(msgs.len(), 1);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        assert_eq!(v["result"], "pong");
        assert_eq!(v["id"], 1);
    }

    #[tokio::test]
    async fn notification_sends_no_response() {
        let (handled, msgs) = run(r#"{"jsonrpc":"2.0","method":"ping"}"#).await;
        assert!(handled);
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn unknown_method_returns_32601() {
        let (handled, msgs) = run(r#"{"jsonrpc":"2.0","method":"nope","id":7}"#).await;
        assert!(handled);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        assert_eq!(v["error"]["code"], -32601);
    }
    #[tokio::test]
    async fn resolve_icon_returns_absolute_path() {
        let (handled, msgs) = run(
            r#"{"jsonrpc":"2.0","method":"resolve_icon","params":{"name":"papirus:folder-open"},"id":4}"#,
        )
        .await;
        assert!(handled);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        let path = v["result"].as_str().expect("result must be a string");
        assert!(path.starts_with('/'));
        if std::path::Path::new("/usr/share/icons/Papirus").exists() {
            assert!(path.contains("/Papirus/"));
        }
    }
    #[tokio::test]
    async fn resolve_icon_passes_absolute_path_through() {
        // regression: do_find dropped its leading-`/` early return, so an
        // already-resolved path fell through to the default placeholder
        let (handled, msgs) = run(
            r#"{"jsonrpc":"2.0","method":"resolve_icon","params":{"name":"/usr/share/icons/Papirus/48x48/apps/github.svg"},"id":8}"#,
        )
        .await;
        assert!(handled);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        assert_eq!(
            v["result"],
            "/usr/share/icons/Papirus/48x48/apps/github.svg"
        );
    }

    #[tokio::test]
    async fn resolve_icon_requires_name() {
        for line in [
            r#"{"jsonrpc":"2.0","method":"resolve_icon","id":5}"#,
            r#"{"jsonrpc":"2.0","method":"resolve_icon","params":{"name":""},"id":6}"#,
            r#"{"jsonrpc":"2.0","method":"resolve_icon","params":{"name":42},"id":7}"#,
        ] {
            let (handled, msgs) = run(line).await;
            assert!(handled);
            let v: Value = serde_json::from_str(&msgs[0]).unwrap();
            assert_eq!(v["error"]["code"], -32602);
        }
    }

    #[tokio::test]
    async fn invalid_params_returns_32602() {
        let (handled, msgs) = run(r#"{"jsonrpc":"2.0","method":"search","params":42,"id":9}"#).await;
        assert!(handled);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        assert_eq!(v["error"]["code"], -32602);
    }
    #[tokio::test]
    async fn search_with_empty_text_returns_32602() {
        // an absent/empty query is not a search; `top` is the most-used method
        let (handled, msgs) = run(r#"{"jsonrpc":"2.0","method":"search","id":10}"#).await;
        assert!(handled);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        assert_eq!(v["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn missing_method_returns_32600() {
        let (handled, msgs) = run(r#"{"jsonrpc":"2.0","params":null,"id":8}"#).await;
        assert!(handled);
        let v: Value = serde_json::from_str(&msgs[0]).unwrap();
        assert_eq!(v["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn non_rpc_line_falls_through() {
        let (handled, msgs) = run("firefox").await;
        assert!(!handled);
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn non_20_object_falls_through() {
        let (handled, msgs) = run(r#"{"method":"ping","id":1}"#).await;
        assert!(!handled);
        assert!(msgs.is_empty());
    }
    #[test]
    fn search_text_accepts_text_object() {
        let p: Value = serde_json::from_str(r#"{"text":"firefox"}"#).unwrap();
        assert_eq!(search_text(&Some(p)).unwrap(), "firefox");
    }

    #[test]
    fn search_text_absent_or_null_means_empty() {
        assert_eq!(search_text(&None).unwrap(), "");
        assert_eq!(search_text(&Some(Value::Null)).unwrap(), "");
    }

    #[test]
    fn search_text_rejects_non_text_params() {
        // bare string and the old `query` alias are no longer accepted
        assert!(search_text(&Some(Value::String("firefox".into()))).is_err());
        let empty: Value = serde_json::from_str("{}").unwrap();
        let query: Value = serde_json::from_str(r#"{"query":"x"}"#).unwrap();
        let num: Value = serde_json::from_str(r#"{"text":42}"#).unwrap();
        assert!(search_text(&Some(empty)).is_err());
        assert!(search_text(&Some(query)).is_err());
        assert!(search_text(&Some(num)).is_err());
    }
    #[test]
    fn select_payload_accepts_item_object() {
        let p: Value = serde_json::from_str(r#"{"title":"x","on_click":"run:ls"}"#).unwrap();
        assert!(select_payload(&Some(p)).unwrap().contains("run:ls"));
        assert!(select_payload(&Some(Value::String("run:ls".into()))).is_err());
    }

    #[test]
    fn forget_key_accepts_on_click_object() {
        let p: Value = serde_json::from_str(r#"{"on_click":"run:ls"}"#).unwrap();
        assert_eq!(forget_key(&Some(p)).unwrap(), "run:ls");
        assert!(forget_key(&Some(Value::String("run:ls".into()))).is_err());
    }

    #[test]
    fn run_cmd_accepts_cmd_object() {
        let p: Value = serde_json::from_str(r#"{"cmd":"echo hi"}"#).unwrap();
        assert_eq!(run_cmd(&Some(p)).unwrap(), "echo hi");
        assert!(run_cmd(&Some(Value::String("echo hi".into()))).is_err());
    }
}

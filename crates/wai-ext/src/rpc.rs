//! Line-delimited JSON-RPC 2.0: one message per line, read with a size cap.

use std::io::{self, BufRead, Read as _};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub(crate) const PARSE_ERROR: i64 = -32700;
pub(crate) const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;
/// wai could not do what was asked (no answer, unknown agent…).
pub(crate) const HOST_ERROR: i64 = -32000;
/// A call the user did not grant: `Permission denied: <permission>`.
pub(crate) const PERMISSION_DENIED: i64 = -32001;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

impl RpcError {
    pub(crate) fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub(crate) enum Line {
    Message(Vec<u8>),
    /// Longer than the cap; the rest of it was skipped, never held.
    TooLarge,
}

/// The next line without its `\n`, holding at most `max` bytes of it. `None` at EOF.
pub(crate) fn read_line(r: &mut impl BufRead, max: usize) -> io::Result<Option<Line>> {
    let mut buf = Vec::new();
    if r.by_ref()
        .take(max as u64 + 1)
        .read_until(b'\n', &mut buf)?
        == 0
    {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(Some(Line::Message(buf)));
    }
    if buf.len() <= max {
        return Ok(Some(Line::Message(buf)));
    }
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(end) => {
                r.consume(end + 1);
                break;
            }
            None => {
                let len = chunk.len();
                r.consume(len);
            }
        }
    }
    Ok(Some(Line::TooLarge))
}

#[derive(Debug)]
pub(crate) enum Incoming {
    /// A request, or a notification when `id` is `None`.
    Call {
        id: Option<Value>,
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Result<Value, RpcError>,
    },
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Incoming, RpcError> {
    let invalid = || RpcError::new(INVALID_REQUEST, "Invalid Request");
    let Value::Object(mut msg) = serde_json::from_slice::<Value>(bytes)
        .map_err(|_| RpcError::new(PARSE_ERROR, "Parse error"))?
    else {
        return Err(invalid());
    };
    let id = msg.remove("id");
    if let Some(method) = msg.remove("method") {
        let Value::String(method) = method else {
            return Err(invalid());
        };
        let params = msg.remove("params").unwrap_or(Value::Null);
        return Ok(Incoming::Call { id, method, params });
    }
    let id = id.ok_or_else(invalid)?;
    let result = match (msg.remove("result"), msg.remove("error")) {
        (_, Some(error)) => Err(serde_json::from_value(error).map_err(|_| invalid())?),
        (Some(result), None) => Ok(result),
        (None, None) => return Err(invalid()),
    };
    Ok(Incoming::Response { id, result })
}

/// A request, or a notification when `id` is `None`.
pub(crate) fn call(id: Option<u64>, method: &str, params: Value) -> Value {
    let mut msg = Map::new();
    msg.insert("jsonrpc".into(), "2.0".into());
    if let Some(id) = id {
        msg.insert("id".into(), id.into());
    }
    msg.insert("method".into(), method.into());
    msg.insert("params".into(), params);
    Value::Object(msg)
}

pub(crate) fn response(id: Value, result: Result<Value, RpcError>) -> Value {
    match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    }
}

/// One line: compact JSON escapes every newline inside strings.
pub(crate) fn encode(msg: &Value) -> Vec<u8> {
    let mut line = serde_json::to_vec(msg).expect("a Value always serializes");
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(input: &[u8], max: usize) -> Vec<Option<Vec<u8>>> {
        let mut r = io::BufReader::with_capacity(4, input);
        std::iter::from_fn(|| read_line(&mut r, max).unwrap())
            .map(|line| match line {
                Line::Message(bytes) => Some(bytes),
                Line::TooLarge => None,
            })
            .collect()
    }

    #[test]
    fn read_line_caps_and_skips() {
        let got = lines(b"12345\n123456789\nab\nlast", 5);
        let want = [Some(&b"12345"[..]), None, Some(b"ab"), Some(b"last")];
        assert_eq!(got, want.map(|l| l.map(<[u8]>::to_vec)));
        assert_eq!(lines(b"123456", 5), [None]);
        assert!(lines(b"", 5).is_empty());
    }

    #[test]
    fn parse_tells_calls_from_responses() {
        let call = parse(br#"{"jsonrpc":"2.0","id":3,"method":"notify","params":{"a":1}}"#);
        assert!(
            matches!(call, Ok(Incoming::Call { id: Some(id), method, .. }) if id == 3 && method == "notify")
        );
        let note = parse(br#"{"jsonrpc":"2.0","method":"panel/render"}"#);
        assert!(matches!(
            note,
            Ok(Incoming::Call {
                id: None,
                params: Value::Null,
                ..
            })
        ));
        let ok = parse(br#"{"jsonrpc":"2.0","id":0,"result":null}"#);
        assert!(matches!(
            ok,
            Ok(Incoming::Response {
                result: Ok(Value::Null),
                ..
            })
        ));
        let err = parse(br#"{"id":0,"error":{"code":-1,"message":"no"}}"#);
        assert!(matches!(err, Ok(Incoming::Response { result: Err(e), .. }) if e.code == -1));
        assert_eq!(parse(b"{").unwrap_err().code, PARSE_ERROR);
        assert_eq!(parse(b"[1]").unwrap_err().code, INVALID_REQUEST);
        assert_eq!(parse(br#"{"id":1}"#).unwrap_err().code, INVALID_REQUEST);
        assert_eq!(parse(br#"{"method":7}"#).unwrap_err().code, INVALID_REQUEST);
    }
}

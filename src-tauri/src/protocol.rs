use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    Ping,
    Analyze,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub kind: RequestKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum ResponseResult {
    Pong { version: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub result: ResponseResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_ping_request_as_single_line() {
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'));
        assert!(line.contains("\"kind\":\"ping\""));
    }

    #[test]
    fn deserializes_pong_response() {
        let line = r#"{"id":"a","result":{"type":"pong","data":{"version":"0.1.0"}}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        assert_eq!(res.id, "a");
        match res.result {
            ResponseResult::Pong { version } => assert_eq!(version, "0.1.0"),
            _ => panic!("expected pong"),
        }
    }

    #[test]
    fn deserializes_error_response() {
        let line = r#"{"id":"a","result":{"type":"error","data":{"message":"boom"}}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        match res.result {
            ResponseResult::Error { message } => assert_eq!(message, "boom"),
            _ => panic!("expected error"),
        }
    }
}

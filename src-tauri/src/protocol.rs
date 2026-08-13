use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    Ping,
    Analyze,
    /// Runs the same per-photo pipeline as `Analyze` but returns per-stage
    /// timings (hash, EXIF, decode, Vision, classical metrics, thumbnail
    /// write) instead of features -- a permanent diagnostic, not scaffolding.
    /// See `scripts/benchmark.sh`.
    Benchmark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub kind: RequestKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// Directory the sidecar should write contact-sheet thumbnails into for
    /// `.analyze` requests. Rust owns `app_data_dir`, so this is computed and
    /// passed in here rather than hardcoded on the Swift side. Renamed to
    /// match `Request.thumbnailDir` in Swift's `Protocol.swift` exactly --
    /// the struct has no blanket `rename_all` the way `RequestKind` does, so
    /// this must be spelled out per-field or the two sides silently
    /// disagree on the wire.
    #[serde(rename = "thumbnailDir", skip_serializing_if = "Option::is_none")]
    pub thumbnail_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum ResponseResult {
    Pong { version: String },
    Error { message: String },
    Analyzed(Vec<serde_json::Value>),
    Benchmarked(Vec<serde_json::Value>),
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
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None, thumbnail_dir: None };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'));
        assert!(line.contains("\"kind\":\"ping\""));
    }

    /// Pins the wire spelling of `thumbnail_dir` as `thumbnailDir` -- it must
    /// match `Request.thumbnailDir` in Swift's `Protocol.swift` exactly, and
    /// there is no blanket `rename_all` on this struct to fall back on.
    #[test]
    fn serializes_thumbnail_dir_as_camel_case() {
        let req = Request {
            id: "a".into(),
            kind: RequestKind::Analyze,
            paths: Some(vec!["/p.jpg".into()]),
            thumbnail_dir: Some("/cache/thumbnails".into()),
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains(r#""thumbnailDir":"/cache/thumbnails""#));
        assert!(!line.contains("thumbnail_dir"));
    }

    /// When absent, `thumbnailDir` must not appear on the wire at all --
    /// matching how `paths` is already omitted for ping requests.
    #[test]
    fn omits_thumbnail_dir_when_absent() {
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None, thumbnail_dir: None };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains("thumbnailDir"));
    }

    #[test]
    fn serializes_benchmark_request_kind_as_lowercase() {
        let req = Request {
            id: "a".into(),
            kind: RequestKind::Benchmark,
            paths: Some(vec!["/p.arw".into()]),
            thumbnail_dir: None,
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains("\"kind\":\"benchmark\""));
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

    /// Pins the actual Swift wire format for `analyze`: a single-field tuple
    /// variant decoding a JSON array under `data`. This is asserted by
    /// `#[serde(tag = "type", content = "data")]` semantics, not tested,
    /// anywhere else — if `Analyzed` ever became a struct variant, or the
    /// `content` key changed, this would still compile and only fail at
    /// runtime against the real sidecar, degrading into `Malformed` errors
    /// that `analyze_batches` then turns into failure records for every
    /// photo in the batch with no clear cause.
    #[test]
    fn deserializes_analyzed_response() {
        let line = r#"{"id":"a","result":{"type":"analyzed","data":[
            {"status":"ok","features":{"phash":"abc123","aestheticScore":0.8,"sharpness":42.0}},
            {"status":"failed","path":"/photos/bad.raw","message":"decode error"}
        ]}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        assert_eq!(res.id, "a");
        match res.result {
            ResponseResult::Analyzed(records) => {
                assert_eq!(records.len(), 2);
                assert_eq!(records[0]["status"], "ok");
                assert_eq!(records[0]["features"]["phash"], "abc123");
                assert_eq!(records[1]["status"], "failed");
                assert_eq!(records[1]["path"], "/photos/bad.raw");
                assert_eq!(records[1]["message"], "decode error");
            }
            other => panic!("expected analyzed, got {other:?}"),
        }
    }

    /// Pins the Swift wire format for `benchmark`: `BenchmarkRecord`'s
    /// `status`/`result` keys (deliberately distinct from `analyze`'s
    /// `status`/`features`, since a benchmark record carries timings, not
    /// photo features) -- see `Benchmarker.swift`.
    #[test]
    fn deserializes_benchmarked_response() {
        let line = r#"{"id":"a","result":{"type":"benchmarked","data":[
            {"status":"ok","result":{"path":"/p.arw","ext":"arw","width":1536,"height":1024,
                "usedFullDecodeFallback":false,
                "timings":{"hashMs":1.0,"exifMs":0.5,"decodeMs":40.0,"visionMs":80.0,
                    "metricsMs":5.0,"thumbnailWriteMs":0.0,"totalMs":126.5}}},
            {"status":"failed","path":"/photos/bad.raw","message":"decode error"}
        ]}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        assert_eq!(res.id, "a");
        match res.result {
            ResponseResult::Benchmarked(records) => {
                assert_eq!(records.len(), 2);
                assert_eq!(records[0]["status"], "ok");
                assert_eq!(records[0]["result"]["ext"], "arw");
                assert_eq!(records[0]["result"]["usedFullDecodeFallback"], false);
                assert_eq!(records[0]["result"]["timings"]["decodeMs"], 40.0);
                assert_eq!(records[1]["status"], "failed");
                assert_eq!(records[1]["path"], "/photos/bad.raw");
            }
            other => panic!("expected benchmarked, got {other:?}"),
        }
    }
}

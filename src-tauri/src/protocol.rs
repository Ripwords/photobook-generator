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
    /// Crops and re-encodes the photos the layout engine placed. See
    /// `Request.export` for the payload and `export::build_items` for how
    /// Rust builds it from a `Book`.
    Export,
    /// Names places: one name or null per `Request.coordinates` entry. The
    /// only request whose data leaves the Mac, as coordinates sent to
    /// Apple's geocoder, so only `commands::place_names` sends it.
    Geocode,
}

/// One photo to crop and write, as decided by the Rust layout engine.
/// Mirrors Swift's `ExportItem` in `Protocol.swift` field for field --
/// `sourcePath`/`cropX`/`cropY`/`cropW`/`cropH` are spelled out individually
/// because, like `Request` below, this struct has no blanket `rename_all`.
///
/// `crop_x/y/w/h` are normalised 0...1 of the photo's own ORIENTED
/// (displayed) frame -- exactly what `book::crop::choose_crop` already
/// produces, so nothing converts on the wire.
///
/// `filename` is the output basename WITHOUT an extension; the sidecar
/// appends the one that matches the format it chooses from the source (see
/// `Exporter.outputFormat` in the Swift sidecar). Sending an extension here
/// would double it or mismatch the sidecar's own choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportItem {
    #[serde(rename = "sourcePath")]
    pub source_path: String,
    pub filename: String,
    #[serde(rename = "cropX")]
    pub crop_x: f64,
    #[serde(rename = "cropY")]
    pub crop_y: f64,
    #[serde(rename = "cropW")]
    pub crop_w: f64,
    #[serde(rename = "cropH")]
    pub crop_h: f64,
}

/// Mirrors Swift's `ExportRequest`. `output_dir` is renamed the same way
/// `Request.thumbnail_dir` is; `items` already matches Swift's spelling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportRequest {
    #[serde(rename = "outputDir")]
    pub output_dir: String,
    pub items: Vec<ExportItem>,
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
    /// Payload for `.export` requests. Nil for every other kind; an
    /// `.export` request without it is answered with an error rather than an
    /// empty result, so a caller that forgets it hears about it. Already
    /// spelled `export` on both sides, so no rename is needed -- but it is
    /// still spelled out explicitly here rather than left to a blanket rule,
    /// matching every other field on this struct.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<ExportRequest>,
    /// Payload for `.geocode` requests, as `[lat, lon]` pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<Vec<crate::book::chapter::LatLon>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum ResponseResult {
    Pong { version: String },
    Error { message: String },
    Analyzed(Vec<serde_json::Value>),
    Benchmarked(Vec<serde_json::Value>),
    /// One `ExportRecord` per `ExportItem`, in input order. Kept as raw JSON
    /// like `Analyzed`/`Benchmarked` rather than a typed Rust enum: nothing
    /// in Rust needs to branch on `.ok` vs `.failed` today, and typing it
    /// early would just be a second place the `type` tag could drift from
    /// Swift's.
    Exported(Vec<serde_json::Value>),
    /// One town name or `None` per coordinate, in request order.
    Geocoded(Vec<Option<String>>),
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
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None, thumbnail_dir: None, export: None, coordinates: None };
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
            export: None,
            coordinates: None,
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains(r#""thumbnailDir":"/cache/thumbnails""#));
        assert!(!line.contains("thumbnail_dir"));
    }

    /// When absent, `thumbnailDir` must not appear on the wire at all --
    /// matching how `paths` is already omitted for ping requests.
    #[test]
    fn omits_thumbnail_dir_when_absent() {
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None, thumbnail_dir: None, export: None, coordinates: None };
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
            export: None,
            coordinates: None,
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

    // --- export: the wire contract read from Protocol.swift ---------------
    //
    // Swift's `ExportItem`/`ExportRequest` have no blanket `rename_all`
    // either, so every field is checked BY NAME. This is the failure mode
    // the brief calls out explicitly: a drifted field name degrades into
    // "every photo failed" with no cause reported anywhere, because Swift's
    // decoder just fails the whole item silently from the caller's point of
    // view. A round-trip test alone (serialize then deserialize with the
    // SAME Rust struct) cannot catch this -- a symmetrically wrong rename
    // round-trips perfectly. Only asserting the literal wire bytes can.

    #[test]
    fn serializes_export_request_kind_as_lowercase() {
        let req = Request {
            id: "a".into(),
            kind: RequestKind::Export,
            paths: None,
            thumbnail_dir: None,
            export: None,
            coordinates: None,
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains("\"kind\":\"export\""));
    }

    /// Every `ExportItem` field, by name, in the exact camelCase spelling
    /// Swift's struct declares. Also asserts the snake_case Rust names never
    /// leak onto the wire -- the mutation this guards against is dropping a
    /// single `#[serde(rename)]` attribute, which compiles fine and only
    /// breaks at the sidecar.
    #[test]
    fn serializes_export_item_fields_in_camel_case() {
        let item = ExportItem {
            source_path: "/photos/a.jpg".into(),
            filename: "p04-z1-abcd1234".into(),
            crop_x: 0.1,
            crop_y: 0.2,
            crop_w: 0.3,
            crop_h: 0.4,
        };
        let line = serde_json::to_string(&item).unwrap();
        assert!(line.contains(r#""sourcePath":"/photos/a.jpg""#), "{line}");
        assert!(line.contains(r#""filename":"p04-z1-abcd1234""#), "{line}");
        assert!(line.contains(r#""cropX":0.1"#), "{line}");
        assert!(line.contains(r#""cropY":0.2"#), "{line}");
        assert!(line.contains(r#""cropW":0.3"#), "{line}");
        assert!(line.contains(r#""cropH":0.4"#), "{line}");
        assert!(!line.contains("source_path"), "{line}");
        assert!(!line.contains("crop_x"), "{line}");
        assert!(!line.contains("crop_y"), "{line}");
        assert!(!line.contains("crop_w"), "{line}");
        assert!(!line.contains("crop_h"), "{line}");
    }

    #[test]
    fn serializes_export_request_output_dir_as_camel_case() {
        let req = ExportRequest { output_dir: "/tmp/out".into(), items: Vec::new() };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains(r#""outputDir":"/tmp/out""#), "{line}");
        assert!(!line.contains("output_dir"), "{line}");
    }

    /// `export` must not appear on the wire at all when the request is not
    /// an export -- matching how `thumbnailDir` is already omitted for ping
    /// requests. A wire-visible `null` here would still be a needless extra
    /// key Swift has to tolerate.
    #[test]
    fn omits_export_field_when_absent() {
        let req = Request {
            id: "a".into(),
            kind: RequestKind::Ping,
            paths: None,
            thumbnail_dir: None,
            export: None,
            coordinates: None,
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains("\"export\""));
    }

    /// The full export request as Swift will actually receive it, checked
    /// key-by-key inside the nested `items` array too -- a rename that only
    /// worked at the top level (e.g. `outputDir` right but `sourcePath`
    /// wrong) would slip past a shallower test.
    #[test]
    fn serializes_a_full_export_request_with_camel_case_items() {
        let req = Request {
            id: "a".into(),
            kind: RequestKind::Export,
            paths: None,
            thumbnail_dir: None,
            export: Some(ExportRequest {
                output_dir: "/tmp/out".into(),
                items: vec![ExportItem {
                    source_path: "/photos/a.jpg".into(),
                    filename: "p04-z1-abcd1234".into(),
                    crop_x: 0.0,
                    crop_y: 0.0,
                    crop_w: 1.0,
                    crop_h: 1.0,
                }],
            }),
            coordinates: None,
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains(r#""outputDir":"/tmp/out""#), "{line}");
        assert!(line.contains(r#""sourcePath":"/photos/a.jpg""#), "{line}");
        assert!(line.contains(r#""cropX":0.0"#), "{line}");
        assert!(!line.contains("output_dir"), "{line}");
        assert!(!line.contains("source_path"), "{line}");
    }

    /// Pins the Swift wire format for `export`: a `type`/`data` tagged union
    /// whose data array holds `ExportRecord`s discriminated on `type`
    /// (`"ok"`/`"failed"`) -- NOT `status`, which an earlier abandoned plan
    /// for this project used and which would silently decode as `Error` under
    /// `ResponseResult`'s own tag if that ever leaked in here.
    #[test]
    fn deserializes_exported_response() {
        let line = r#"{"id":"a","result":{"type":"exported","data":[
            {"type":"ok","path":"/out/p04-z1-abcd1234.jpg","width":3000,"height":2000,"bytes":845210},
            {"type":"failed","filename":"p04-z2-ef567890","message":"could not decode: /photos/bad.raw"}
        ]}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        assert_eq!(res.id, "a");
        match res.result {
            ResponseResult::Exported(records) => {
                assert_eq!(records.len(), 2);
                assert_eq!(records[0]["type"], "ok");
                assert_eq!(records[0]["path"], "/out/p04-z1-abcd1234.jpg");
                assert_eq!(records[0]["width"], 3000);
                assert_eq!(records[1]["type"], "failed");
                assert_eq!(records[1]["filename"], "p04-z2-ef567890");
            }
            other => panic!("expected exported, got {other:?}"),
        }
    }

    #[test]
    fn serializes_a_geocode_request_as_lat_lon_pairs() {
        use crate::book::chapter::LatLon;
        let req = Request {
            id: "g".into(),
            kind: RequestKind::Geocode,
            paths: None,
            thumbnail_dir: None,
            export: None,
            coordinates: Some(vec![LatLon::new(35.01, 135.77).unwrap(), LatLon::new(64.15, -21.94).unwrap()]),
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains(r#""kind":"geocode""#), "{line}");
        assert!(line.contains(r#""coordinates":[[35.01,135.77],[64.15,-21.94]]"#), "{line}");
    }

    #[test]
    fn omits_coordinates_when_absent() {
        let req = Request { id: "a".into(), kind: RequestKind::Ping, paths: None, thumbnail_dir: None, export: None, coordinates: None };
        assert!(!serde_json::to_string(&req).unwrap().contains("coordinates"));
    }

    /// Swift answers a failed lookup with a null in that position. A reader
    /// that dropped nulls would slide every later name onto the wrong chapter.
    #[test]
    fn deserializes_geocoded_names_keeping_each_null_in_place() {
        let line = r#"{"id":"g","result":{"type":"geocoded","data":[null,"Kyoto",null,"Reykjavík"]}}"#;
        let res: Response = serde_json::from_str(line).unwrap();
        match res.result {
            ResponseResult::Geocoded(names) => assert_eq!(
                names,
                vec![None, Some("Kyoto".to_string()), None, Some("Reykjavík".to_string())]
            ),
            other => panic!("expected geocoded, got {other:?}"),
        }
    }
}

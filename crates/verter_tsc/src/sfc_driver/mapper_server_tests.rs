//! The wire server driven through the protocol's request sequence in process.

use std::io::Cursor;

use serde_json::{json, Value};
use verter_tsgo_api::jsonrpc::{encode_message, MessageFramer};

use super::serve;
use crate::sfc_driver::exit::ExitStatus;

const TS_CARRIER: &str = concat!(
    "<template>\n  <p>{{ count }}</p>\n</template>\n",
    "<script setup lang=\"ts\">\n",
    "const count: number = 1\n",
    "</script>\n",
);

const TSX_CARRIER: &str = concat!(
    "<template>\n  <p>{{ count }}</p>\n</template>\n",
    "<script setup lang=\"tsx\">\n",
    "const count: number = 1\n",
    "</script>\n",
);

fn request(id: u64, method: &str, params: Value) -> Vec<u8> {
    encode_message(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
}

/// Serve `requests` and return the exit status and every reply, by id.
fn exchange(requests: &[Vec<u8>]) -> (ExitStatus, Vec<Value>) {
    let mut input = Cursor::new(requests.concat());
    let mut output = Vec::new();
    let status = serve(&mut input, &mut output);
    let mut framer = MessageFramer::new();
    framer.push(&output);
    let mut replies = Vec::new();
    while let Some(reply) = framer.next_message().expect("well-formed replies") {
        replies.push(reply);
    }
    (status, replies)
}

fn transform(id: u64, file_name: &str, content: &str) -> Vec<u8> {
    request(
        id,
        "transform",
        json!({ "projectHandle": "p1", "fileName": file_name, "content": content }),
    )
}

#[test]
fn the_protocol_sequence_is_answered_for_the_ts_and_tsx_dialects() {
    let notification = encode_message(&json!({ "jsonrpc": "2.0", "method": "$/ignored" }));
    let (status, replies) = exchange(&[
        request(
            1,
            "initialize",
            json!({ "positionEncodings": ["utf-16", "utf-8"], "locale": "en" }),
        ),
        notification,
        request(
            2,
            "openProject",
            json!({
                "projectHandle": "p1",
                "configFileName": "/ws/tsconfig.json",
                "compilerOptions": { "strict": true },
            }),
        ),
        transform(3, "/ws/src/A.vue", TS_CARRIER),
        transform(4, "/ws/src/B.vue", TSX_CARRIER),
        request(5, "closeProject", json!({ "projectHandle": "p1" })),
        transform(6, "/ws/src/A.vue", TS_CARRIER),
    ]);
    assert_eq!(status, ExitStatus::Clean);
    let ids: Vec<u64> = replies
        .iter()
        .map(|reply| reply["id"].as_u64().expect("numeric id"))
        .collect();
    assert_eq!(
        ids,
        [1, 2, 3, 4, 5, 6],
        "every request and no notification is answered"
    );

    assert_eq!(replies[0]["result"]["positionEncoding"], "utf-8");
    assert_eq!(replies[0]["result"]["diagnosticSource"], "verter");
    assert_eq!(replies[1]["result"]["optionDiagnostics"], json!([]));
    assert!(replies[1]["result"].get("configIdentity").is_none());

    for (reply, extension, content) in [
        (&replies[2], ".ts", TS_CARRIER),
        (&replies[3], ".tsx", TSX_CARRIER),
    ] {
        let result = &reply["result"];
        assert_eq!(result["extension"], extension, "{reply}");
        let text = result["text"].as_str().expect("text");
        assert!(text.contains("const count: number = 1"), "{text}");
        let mappings = result["mappings"].as_array().expect("mappings");
        let verbatim = mappings
            .iter()
            .filter_map(|mapping| {
                let row: Vec<u64> = mapping
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_u64)
                    .collect();
                (row.len() == 6 && row[4] == 0).then_some(row)
            })
            .find(|row| {
                let virtual_text = &text[row[0] as usize..(row[0] + row[1]) as usize];
                virtual_text.contains("const count: number = 1")
            })
            .expect("the setup statement has a verbatim mapping");
        assert_eq!(
            &text[verbatim[0] as usize..(verbatim[0] + verbatim[1]) as usize],
            &content[verbatim[2] as usize..(verbatim[2] + verbatim[3]) as usize],
        );
    }

    assert_eq!(replies[4]["result"], Value::Null);
    assert_eq!(
        replies[5]["error"]["code"], -32602,
        "a closed project is unknown"
    );
}

#[test]
fn an_encoding_the_mapper_cannot_speak_is_refused() {
    let (status, replies) = exchange(&[request(
        1,
        "initialize",
        json!({ "positionEncodings": ["utf-16"] }),
    )]);
    assert_eq!(status, ExitStatus::Clean);
    assert_eq!(replies[0]["error"]["code"], -32602);
}

#[test]
fn an_unknown_method_is_an_error_and_a_malformed_frame_ends_the_session() {
    let (status, replies) = exchange(&[
        request(1, "nothing", Value::Null),
        b"Content-Length: nope\r\n\r\n{}".to_vec(),
    ]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["error"]["code"], -32601);
    assert_eq!(status, ExitStatus::Failed);
}

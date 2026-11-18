//! The content mapper's wire server: the projection adapter served over
//! standard input and output. Its message types are in
//! `verter_session::sfc_projection_mapper`.
//!
//! JSON-RPC 2.0 in `Content-Length` frames, the framing the engine's own
//! protocol connections use. The engine sends every request and the server
//! answers each one; it sends nothing of its own. A refusal of one carrier is
//! a diagnostic inside a `transform` result, never an error response.

use std::io::{ErrorKind, Read, Write};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use verter_session::sfc_projection_mapper::{
    CloseProjectParams, ContentMapperProjectionAdapter, InitializeParams, InitializeResult,
    MapperError, MapperProjectHandle, OpenProjectParams, OpenProjectResult, TransformParams,
    MAPPER_DIAGNOSTIC_SOURCE,
};
use verter_tsgo_api::jsonrpc::{encode_message, MessageFramer};

use super::exit::ExitStatus;

/// JSON-RPC: the method does not exist.
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: the parameters are not the method's.
const INVALID_PARAMS: i64 = -32602;
/// JSON-RPC: the server could not answer a well-formed request.
const INTERNAL_ERROR: i64 = -32603;

/// Bytes read from the input per call.
const READ_CHUNK: usize = 64 * 1024;

/// A JSON-RPC error: its code and message.
type RpcError = (i64, String);

/// Serve mapper requests from `stdin` until it closes, answering on `stdout`.
///
/// `Clean` when the input ends between frames; `Failed` when a frame is
/// malformed or either stream fails, since the conversation cannot continue.
pub fn serve(stdin: &mut dyn Read, stdout: &mut dyn Write) -> ExitStatus {
    let adapter = ContentMapperProjectionAdapter::new();
    let mut framer = MessageFramer::new();
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let read = match stdin.read(&mut chunk) {
            Ok(0) => return ExitStatus::Clean,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return ExitStatus::Failed,
        };
        framer.push(&chunk[..read]);
        loop {
            let message = match framer.next_message() {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(_) => return ExitStatus::Failed,
            };
            let Some(reply) = answer(&adapter, &message) else {
                continue;
            };
            if stdout
                .write_all(&encode_message(&reply))
                .and_then(|()| stdout.flush())
                .is_err()
            {
                return ExitStatus::Failed;
            }
        }
    }
}

/// The reply to one message: `None` for a notification or anything that is
/// not a request, since only a request with an id is answered.
fn answer(adapter: &ContentMapperProjectionAdapter, message: &Value) -> Option<Value> {
    let method = message.get("method")?.as_str()?;
    let id = message.get("id").filter(|id| !id.is_null())?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match dispatch(adapter, method, params) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, text)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": text },
        }),
    })
}

fn dispatch(
    adapter: &ContentMapperProjectionAdapter,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => {
            let params: InitializeParams = parse(params)?;
            let position_encoding = params.negotiated_encoding().ok_or_else(|| {
                (
                    INVALID_PARAMS,
                    format!(
                        "the mapper speaks utf-8 positions only; offered: {:?}",
                        params.position_encodings
                    ),
                )
            })?;
            to_value(&InitializeResult {
                position_encoding,
                diagnostic_source: MAPPER_DIAGNOSTIC_SOURCE,
            })
        }
        "openProject" => {
            let (handle, settings) = parse::<OpenProjectParams>(params)?.into_project();
            adapter
                .open_project(handle, settings)
                .map_err(mapper_error)?;
            to_value(&OpenProjectResult::default())
        }
        "transform" => {
            let params: TransformParams = parse(params)?;
            let handle = MapperProjectHandle(params.project_handle.into());
            let result = adapter
                .transform(&handle, &params.file_name, &params.content)
                .map_err(mapper_error)?;
            let extension = adapter.virtual_extension(result.dialect).ok_or_else(|| {
                (
                    INTERNAL_ERROR,
                    format!(
                        "no checking-module extension for the {} dialect",
                        result.dialect.extension()
                    ),
                )
            })?;
            to_value(&result.to_wire(extension))
        }
        "closeProject" => {
            let params: CloseProjectParams = parse(params)?;
            adapter
                .close_project(&MapperProjectHandle(params.project_handle.into()))
                .map_err(mapper_error)?;
            Ok(Value::Null)
        }
        _ => Err((METHOD_NOT_FOUND, format!("method not found: {method}"))),
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|error| (INVALID_PARAMS, error.to_string()))
}

fn to_value(result: &impl serde::Serialize) -> Result<Value, RpcError> {
    serde_json::to_value(result).map_err(|error| (INTERNAL_ERROR, error.to_string()))
}

fn mapper_error(error: MapperError) -> RpcError {
    match error {
        MapperError::UnknownProject(handle) => (
            INVALID_PARAMS,
            format!("unknown projectHandle {}", handle.0),
        ),
        MapperError::ProjectAlreadyOpen(handle) => (
            INVALID_PARAMS,
            format!("projectHandle {} is already open", handle.0),
        ),
    }
}

#[cfg(test)]
#[path = "mapper_server_tests.rs"]
mod tests;

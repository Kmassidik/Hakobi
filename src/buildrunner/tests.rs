use serde_json::Value;

use super::{
    payload::{BuildKind, BuildPayload},
    BuildRequest,
};

mod client;
mod payload;
mod response;

fn sample_request() -> BuildRequest {
    BuildRequest {
        git_url: "https://github.com/acme/app.git".to_owned(),
        branch: "main".to_owned(),
        image_tag: "acme/app:latest".to_owned(),
        dockerfile_path: None,
        context_dir: None,
        token: None,
    }
}

fn payload_value(kind: BuildKind, request: &BuildRequest) -> Value {
    serde_json::to_value(BuildPayload::for_request(kind, request).unwrap()).unwrap()
}

fn rejection(kind: BuildKind, request: &BuildRequest) -> String {
    BuildPayload::for_request(kind, request)
        .err()
        .expect("request should have been rejected")
        .to_string()
}

use crate::errors::VoipBitsError;
use fehler::{throw, throws};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "body")]
    pub body: Option<String>,

    #[serde(rename = "httpMethod")]
    pub http_method: String,

    #[serde(rename = "path")]
    pub path: String,

    #[serde(rename = "pathParameters")]
    pub path_parameters: Option<serde_json::Value>,

    #[serde(rename = "queryStringParameters")]
    pub query_string_parameters: Option<HashMap<String, String>>,

    #[serde(rename = "requestContext")]
    pub request_context: RequestContext,

    #[serde(rename = "resource")]
    pub resource: String,
}

impl Event {
    #[throws(VoipBitsError)]
    pub fn get_qs(&self, key: &str) -> &str {
        if let Some(ref qs) = self.query_string_parameters {
            match qs.get(key) {
                Some(v) => v.as_ref(),
                None => throw!(VoipBitsError::MissingParameter(key.to_string())),
            }
        } else {
            throw!(VoipBitsError::MissingParameter(key.to_string()));
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Headers {
    #[serde(rename = "Accept")]
    pub accept: Option<String>,

    #[serde(rename = "Accept-Encoding")]
    pub accept_encoding: Option<String>,

    #[serde(rename = "Host")]
    pub host: String,

    #[serde(rename = "User-Agent")]
    pub user_agent: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestContext {
    #[serde(rename = "domainName")]
    pub domain_name: String,

    #[serde(rename = "httpMethod")]
    pub http_method: String,

    #[serde(rename = "path")]
    pub path: String,

    #[serde(rename = "protocol")]
    pub protocol: String,

    #[serde(rename = "requestId")]
    pub request_id: String,

    #[serde(rename = "requestTime")]
    pub request_time: String,

    #[serde(rename = "requestTimeEpoch")]
    pub request_time_epoch: i64,

    #[serde(rename = "resourceId")]
    pub resource_id: String,

    #[serde(rename = "resourcePath")]
    pub resource_path: String,

    #[serde(rename = "stage")]
    pub stage: String,
}

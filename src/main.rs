mod acrobits;
mod errors;
mod push_manager;
mod voipms;

use crate::acrobits::Acrobits;
use crate::push_manager::PushManager;
use crate::voipms::VoipMS;
use axum::{
    body::{Body, Bytes},
    extract::{Extension, Query},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use hyper::{Method, Uri};
use lambda_web::{is_running_on_lambda, run_hyper_on_lambda, LambdaError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use structopt::StructOpt;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, StructOpt)]
#[structopt(name = "voipbits", about = "This is VoipBits")]
pub struct Opt {
    #[structopt(env)]
    private_key: String,

    #[structopt(env, default_value = "https://voipbits.wooya.me")]
    server_url: String,
}

impl Opt {
    pub fn report_url(&self) -> String {
        format!(
            "{url}/report?token=%pushToken%&appid=%pushappid%&selector=%selector%",
            url = self.server_url
        )
    }

    pub fn fetch_url(&self) -> String {
        format!(
            "{url}/fetch?last_id=%last_known_sms_id%",
            url = self.server_url
        )
    }

    pub fn send_url(&self) -> String {
        format!(
            "{url}/send?to=%sms_to%&body=%sms_body%",
            url = self.server_url
        )
    }

    #[allow(unused)]
    pub fn provision_url(&self) -> String {
        format!("{url}/provision", url = self.server_url)
    }

    pub fn notify_url(&self) -> String {
        format!(
            "{url}/notify?message={{MESSAGE}}&from={{FROM}}&to={{TO}}",
            url = self.server_url
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), LambdaError> {
    tracing_subscriber::fmt::init();

    let opt = Opt::from_args();

    // build our application with a route
    let app = Router::new()
        .route("/send", post(send))
        .route("/notify", get(notify))
        .route("/provision", post(provision))
        .route("/fetch", post(fetch))
        .route("/report", post(report))
        .layer(middleware::from_fn(print_request_response))
        .layer(Extension(opt));

    if is_running_on_lambda() {
        // Run app on AWS Lambda
        run_hyper_on_lambda(app).await?;
    } else {
        // Run app on local server
        let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;
    }
    Ok(())
}

async fn print_request_response(
    req: Request<Body>,
    next: Next<Body>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = req.into_parts();
    let bytes = buffer_and_print_request(&parts.method, &parts.uri, body).await?;
    let req = Request::from_parts(parts, Body::from(bytes));

    let res = next.run(req).await;

    let (parts, body) = res.into_parts();
    let bytes = buffer_and_print_response(parts.status, body).await?;
    let res = Response::from_parts(parts, Body::from(bytes));

    Ok(res)
}

async fn buffer_and_print_request<B>(
    method: &Method,
    uri: &Uri,
    body: B,
) -> Result<Bytes, (StatusCode, String)>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let bytes = match hyper::body::to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("[request] failed to read body: {}", err),
            ));
        }
    };

    if let Ok(body) = std::str::from_utf8(&bytes) {
        debug!(
            "[request] method={}, uri={:?}, body={:?}",
            method, uri, body
        );
    }

    Ok(bytes)
}

async fn buffer_and_print_response<B>(
    status: StatusCode,
    body: B,
) -> Result<Bytes, (StatusCode, String)>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let bytes = match hyper::body::to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("[response] failed to read body: {}", err),
            ));
        }
    };

    if let Ok(body) = std::str::from_utf8(&bytes) {
        debug!("[response] code = {:?}, body = {:?}", status, body);
    }

    Ok(bytes)
}

/* -------------------------------- Handlers -------------------------------- */
#[derive(Deserialize, Debug)]
struct SendQuery {
    to: String,
    body: String,
}

#[tracing::instrument(skip(opt))]
async fn send(
    Extension(opt): Extension<Opt>,
    query: Query<SendQuery>,
    cred: String,
) -> Json<Value> {
    let to = &query.to;
    let body = &query.body;
    let voipms = VoipMS::from_cred(&opt.private_key, &cred).unwrap();

    info!(
        "[send] Sending message ({} -> {}) '{}'",
        voipms.did, to, body
    );
    let ret_ids = voipms.send_sms(to, body).await.unwrap();

    Json(json!({
        "sms_id": ret_ids[0]
    }))
}

#[tracing::instrument(skip(opt))]
async fn provision(Extension(opt): Extension<Opt>, cred: String) -> impl IntoResponse {
    // cred is in <did>:<account>:<password> form

    let voipms = VoipMS::from_cred(&opt.private_key, &cred).unwrap();

    voipms.set_sms_callback(&opt).await.unwrap();
    info!("Provisioning for {}", voipms.did);

    let xml = format!(
        "<account>
            <pushTokenReporterUrl>{}</pushTokenReporterUrl>
            <pushTokenReporterPostData>{cred}</pushTokenReporterPostData>
            <pushTokenReporterContentType>text/plain</pushTokenReporterContentType>

            <genericSmsFetchUrl>{}</genericSmsFetchUrl>
            <genericSmsFetchPostData>{cred}</genericSmsFetchPostData>
            <genericSmsFetchContentType>text/plain</genericSmsFetchContentType>

            <genericSmsSendUrl>{}</genericSmsSendUrl>
            <genericSmsPostData>{cred}</genericSmsPostData>
            <genericSmsContentType>text/plain</genericSmsContentType>
            
            <voipmsNotificationUrl>{}</voipmsNotificationUrl>
            <allowMessage>1</allowMessage>
            <voiceMailNumber>*97</voiceMailNumber>
        </account>",
        opt.report_url().replace("&", "&amp;"),
        opt.fetch_url().replace("&", "&amp;"),
        opt.send_url().replace("&", "&amp;"),
        opt.notify_url().replace("&", "&amp;"),
        cred = cred,
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml")],
        xml,
    )
}

#[derive(Deserialize, Debug)]
struct ReportQuery {
    token: String,
    appid: String,
    selector: String,
}

#[tracing::instrument(skip(opt))]
async fn report(Extension(opt): Extension<Opt>, query: Query<ReportQuery>, cred: String) {
    // cred is in <did>:<account>:<password> form

    let push_token = &query.token;
    let appid = &query.appid;
    let selector = &query.selector;
    if push_token.trim().len() == 0 {
        // Sometimes acrobits gives you empty push token, we just ignore it.
        return;
    }
    let voipms = VoipMS::from_cred(&opt.private_key, &cred).unwrap();
    info!("[report] New report for {}", voipms.did);

    PushManager::new()
        .await
        .save_token(&voipms.did, appid, push_token, selector)
        .await
        .unwrap();
}

#[derive(Deserialize, Debug)]
struct NotifyQuery {
    message: String,
    from: String,
    to: String,
}

#[tracing::instrument]
async fn notify(query: Query<NotifyQuery>) -> &'static str {
    let message = &query.message;
    let did = &query.to;
    let from = &query.from;

    info!("New message {} -> {}: '{}'", from, did, message);

    let acrobits = Acrobits::new();

    let pm = PushManager::new().await;

    let tokens = pm.get_tokens(did).await.unwrap();

    let mut failed_tokens = vec![];
    for (appid, push_token, selector) in tokens {
        if let Err(e) = acrobits
            .notify(&appid, &push_token, &selector, from, message)
            .await
        {
            warn!(
                "Notify device error: {:?}, removing the push token {}",
                e, push_token
            );
            failed_tokens.push((appid, push_token, selector));
        }
    }

    pm.remove_tokens(did, &failed_tokens).await.unwrap();

    "ok"
}

#[derive(Deserialize, Debug)]
struct FetchQuery {
    last_id: Option<String>,
}

#[tracing::instrument(skip(opt))]
async fn fetch(
    Extension(opt): Extension<Opt>,
    query: Query<FetchQuery>,
    cred: String,
) -> Json<Value> {
    let voipms = VoipMS::from_cred(&opt.private_key, &cred).unwrap();

    let payload = match query.last_id {
        Some(ref last_id) => {
            // Fetching last ID, which means acrobits already have the messages sent by us.
            // So we only return the incoming messages
            let mut smss = voipms.fetch_sms_after_id(last_id).await.unwrap();
            smss.retain(|sms| sms.recipient.is_none() && &sms.sms_id > last_id);
            smss
        }
        None => voipms.fetch_sms_from_date(None).await.unwrap(),
    };
    info!("[fetch] Total {} SMS", payload.len());

    let (sent, received): (Vec<_>, Vec<_>) =
        payload.into_iter().partition(|sms| sms.recipient.is_some());

    let body = json!({
        "date": Utc::now().to_rfc3339(),
        "received_smss": received,
        "sent_smss": sent,
    });

    Json(body)
}

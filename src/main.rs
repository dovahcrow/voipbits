mod acrobits;
mod errors;
mod event;
mod push_manager;
mod voipms;

use crate::acrobits::Acrobits;
use crate::errors::VoipBitsError;
use crate::push_manager::PushManager;
use crate::voipms::VoipMS;
use chrono::Utc;
use env_logger::init;
use event::Event;
use failure::Error;
use fehler::throws;
use lambda::{handler_fn, Context};
use log::{error, info, warn};
use serde_json::{json, to_string, Value};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "voipbits", about = "This is VoipBits")]
pub struct Opt {
    #[structopt(env, default_value = "https://voipbits.wooya.me")]
    server_url: String,

    #[structopt(env)]
    public_key: String,

    #[structopt(env)]
    private_key: String,
}

impl Opt {
    pub fn report_url(&self) -> String {
        format!("{url}/report?token=%pushToken%&appid=%pushappid%&selector=%selector%", url = self.server_url)
    }

    pub fn fetch_url(&self) -> String {
        format!("{url}/fetch?last_id=%last_known_sms_id%", url = self.server_url)
    }

    pub fn send_url(&self) -> String {
        format!("{url}/send?to=%sms_to%&body=%sms_body%", url = self.server_url)
    }

    #[allow(unused)]
    pub fn provision_url(&self) -> String {
        format!("{url}/provision", url = self.server_url)
    }

    pub fn notify_url(&self) -> String {
        format!("{url}/notify?message={{MESSAGE}}&from={{FROM}}&to={{TO}}", url = self.server_url)
    }
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    init();
    let func = handler_fn(func);
    if let Err(e) = lambda::run(func).await {
        error!("Lambda fail: {:?}", e);
    }
    Ok(())
}

#[throws(Error)]
async fn func(event: Event, _: Context) -> Value {
    info!("Incoming event: {}", to_string(&event).unwrap());

    match process(event).await {
        Ok(None) => {
            info!("Exit with ok");
            json!({
                "statusCode": 200,
                "body": "{\"status\": \"ok\"}"
            })
        }
        Ok(Some(resp)) => {
            info!("Exit with return {}", to_string(&resp).unwrap());
            resp
        }
        Err(e) => {
            error!("Exit with error {:?}", e);
            json!({
                "statusCode": 400,
                "body": format!("{{\"error\": \"{}\"}}", e)
            })
        }
    }
}

#[throws(Error)]
async fn process(event: Event) -> Option<Value> {
    let opt = Opt::from_args();
    match &event.path[..] {
        "/send" => return Some(send(&opt.private_key, &event).await?),
        "/notify" => notify(&opt, &event).await?,
        "/provision" => return Some(provision(&opt, &event).await?),
        "/report" => report(&opt, &event).await?,
        "/fetch" => return Some(fetch(&opt, &event).await?),
        path => unreachable!("Unexpected path {}", path),
    }
    None
}

// Actions

#[throws(Error)]
async fn send(priv_key: &str, event: &Event) -> Value {
    let cred = event.body.as_ref().ok_or(VoipBitsError::MissingAccountInfo)?;
    let to = event.get_qs("to")?;
    let body = event.get_qs("body")?;
    let voipms = VoipMS::from_cred(priv_key, cred)?;

    info!("[send] Sending message ({} -> {}) '{}'", voipms.did, to, body);
    let ret_ids = voipms.send_sms(to, body).await?;

    json!({
        "statusCode": 200,
        "headers": {
            "Content-Type": "application/json"
        },
        "body": to_string(&json!({
            "sms_id": ret_ids[0]
        })).unwrap(),
    })
}

#[throws(Error)]
async fn provision(opt: &Opt, event: &Event) -> Value {
    // cred is in <did>:<account>:<password> form
    let cred = event.body.as_ref().ok_or(VoipBitsError::MissingAccountInfo)?;
    let voipms = VoipMS::from_cred(&opt.private_key, &cred)?;

    voipms.set_sms_callback(&opt).await?;
    info!("[provision] Provisioning for {}", voipms.did);

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

    json!({
        "statusCode": 200,
        "headers": {
            "Content-Type": "application/xml"
        },
        "body": xml,
    })
}

#[throws(Error)]
async fn report(opt: &Opt, event: &Event) {
    // cred is in <did>:<account>:<password> form
    let cred = event.body.as_ref().ok_or(VoipBitsError::MissingAccountInfo)?;
    let push_token = event.get_qs("token")?;
    let appid = event.get_qs("appid")?;
    let selector = event.get_qs("selector")?;
    if push_token.trim().len() == 0 {
        // Sometimes acrobits gives you empty push token, we just ignore it.
        return;
    }
    let voipms = VoipMS::from_cred(&opt.private_key, cred)?;
    info!("[report] New report for {}", voipms.did);

    PushManager::new().save_token(&voipms.did, appid, push_token, selector).await?;
}

#[throws(Error)]
async fn notify(_: &Opt, event: &Event) {
    let message = event.get_qs("message")?;
    let did = event.get_qs("to")?;
    let from = event.get_qs("from")?;

    info!("[notify] New message {} -> {}: '{}'", from, did, message);

    let acrobits = Acrobits::new();

    let pm = PushManager::new();

    let tokens = pm.get_tokens(did).await?;

    let mut failed_tokens = vec![];
    for (appid, push_token, selector) in tokens {
        if let Err(e) = acrobits.notify(&appid, &push_token, &selector, from, message).await {
            warn!("[notify] Notify device error: {:?}, removing the push token {}", e, push_token);
            failed_tokens.push((appid, push_token, selector));
        }
    }

    pm.remove_tokens(did, &failed_tokens).await?;
}

#[throws(Error)]
async fn fetch(opt: &Opt, event: &Event) -> Value {
    let cred = event.body.as_ref().ok_or(VoipBitsError::MissingAccountInfo)?;
    let voipms = VoipMS::from_cred(&opt.private_key, cred)?;

    let payload = match event.get_qs("last_id") {
        Ok(last_id) => {
            // Fetching last ID, which means acrobits already have the messages sent by us.
            // So we only return the incoming messages
            let mut smss = voipms.fetch_sms_after_id(last_id).await?;
            smss.retain(|sms| sms.recipient.is_none() && sms.sms_id.as_str() > last_id);
            smss
        }
        Err(_) => voipms.fetch_sms_from_date(None).await?,
    };
    info!("[fetch] Total {} SMS", payload.len());

    let (sent, received): (Vec<_>, Vec<_>) = payload.into_iter().partition(|sms| sms.recipient.is_some());

    let body = json!({
        "date": Utc::now().to_rfc3339(),
        "received_smss": received,
        "sent_smss": sent,
    });

    json!({
        "statusCode": 200,
        "headers": {
            "Content-Type": "application/json"
        },
        "body": to_string(&body).unwrap()
    })
}

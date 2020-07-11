use crate::errors::VoipBitsError;
use crate::Opt;
use chrono::{DateTime, Duration, NaiveDateTime, Timelike, Utc};
use chrono_tz::US::Pacific;
use failure::Error;
use fehler::{throw, throws};
use lazy_static::lazy_static;
use log::{error, info};
use maplit::hashmap;
use regex::Regex;
use reqwest::Client;
use rsa::{PaddingScheme, RSAPrivateKey};
use rusoto_core::Region;
use rusoto_dynamodb::DynamoDbClient;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{from_str, Value};
use std::str;

const VOIPMS_URL: &'static str = "https://www.voip.ms/api/v1/rest.php";

lazy_static! {
    static ref DYNAMO: DynamoDbClient = DynamoDbClient::new(Region::UsWest2);
}

pub struct VoipMS {
    user: String,
    key: String,
    pub did: String,
    client: Client,
}

impl VoipMS {
    #[throws(Error)]
    pub fn from_cred(priv_key: &str, cred: &str) -> VoipMS {
        let cred = cred.replace(" ", "+");
        let priv_key = RSAPrivateKey::from_pkcs8(&base64::decode(priv_key)?)?;
        let cred = priv_key.decrypt(PaddingScheme::new_pkcs1v15_encrypt(), &base64::decode(cred)?)?;

        let cred = String::from_utf8_lossy(&cred);
        let creds: Vec<_> = cred.split(":").collect();
        match creds.as_slice() {
            [did, username, password] => VoipMS::new(&username, &password, did),
            _ => unreachable!(),
        }
    }

    pub fn new(user: &str, key: &str, did: &str) -> VoipMS {
        Self {
            user: user.into(),
            key: key.into(),
            did: did.into(),
            client: Client::new(),
        }
    }

    #[throws(Error)]
    pub async fn request<'a, T, O>(&'a self, params: T) -> O
    where
        T: IntoIterator<Item = (&'a str, &'a str)>,
        O: DeserializeOwned,
    {
        let mut qs = hashmap! {
            "api_username" => self.user.as_ref(),
            "api_password" => self.key.as_ref(),
            "did" => self.did.as_ref(),
        };

        qs.extend(params);

        let resp = self.client.get(VOIPMS_URL).query(&qs).send().await?;
        let status = resp.status();
        let payload = resp.text().await?;

        if !status.is_success() {
            error!("[Voip.ms] Response: ({}) {}", status, payload);
        } else {
            info!("[Voip.ms] Response: ({}) {}", status, payload);
        }

        from_str(&payload)?
    }

    #[throws(Error)]
    pub async fn send_sms(&self, dst: &str, msg: &str) -> Vec<String> {
        // Clean up number and message text
        let re = Regex::new(r"\D").unwrap();
        let mut dst = re.replace_all(dst, "").to_owned().to_string();
        let mut msg = msg.trim();
        // Remove leading '1' on 11-digit phone numbers
        if dst.len() == 11 {
            dst = dst.trim_start_matches('1').to_string();
        }

        // Validate destination number and message text
        if dst.len() != 10 {
            throw!(VoipBitsError::InvalidNumber(dst.into()));
        }
        if msg.len() == 0 {
            throw!(VoipBitsError::EmptyMessage);
        }

        // Recursively send 160 character chunks
        let mut ids = vec![]; // sent message ids
        while msg.len() != 0 {
            let mut end = msg.len().min(160);
            while !msg.is_char_boundary(end) {
                end = end.checked_sub(1).expect("end underflow");
            }

            info!("Sending piece {}", &msg[..end]);
            let resp: VoipSendSMSResponse = self
                .request(hashmap! {
                    "method" => "sendSMS",
                    "dst" => &dst,
                    "message" => &msg[..end]
                })
                .await?;
            ids.push(resp.sms.to_string());
            msg = &msg[end..];
        }
        ids
    }

    #[throws(Error)]
    pub async fn fetch_sms_after_id(&self, id: &str) -> Vec<AcrobitsSMS> {
        let resp: VoipGetSMSResponse = self
            .request(hashmap! {
                "method" => "getSMS",
                "sms" => id,
                "limit" => "1",
                "timezone" => if is_dst() { "-1" } else { "0" },
            })
            .await?;

        if matches!(resp.sms, None) {
            return vec![];
        }

        let date = match resp.sms.unwrap().as_slice() {
            [] => throw!(VoipBitsError::NoSuchSMS(id.into())),
            [sms] => sms.date,
            [..] => unreachable!("Multiple SMS with same ID"),
        };

        info!("[Voip.ms] Date of SMS {}: {}", id, date);
        self.fetch_sms_from_date(Some(date)).await?
    }

    #[throws(Error)]
    pub async fn fetch_sms_from_date(&self, from: Option<DateTime<Utc>>) -> Vec<AcrobitsSMS> {
        // Query voip.ms for received SMS messages ranging from 90 days ago to tomorrow
        let from = from.unwrap_or_else(|| Utc::now() - Duration::days(90)).format("%Y-%m-%d").to_string();
        let to = (Utc::now() + Duration::days(1)).format("%Y-%m-%d").to_string();

        info!("[Voip.ms] Getting SMS from {} to {}", from, to);

        let resp: VoipGetSMSResponse = self
            .request(hashmap! {
                "method" => "getSMS",
                "from" => &from,
                "to" => &to,
                "limit" => "9999",
                "timezone" => if is_dst() { "-1" } else { "0" },
            })
            .await?;
        if resp.sms.is_none() {
            return vec![];
        }

        resp.sms.unwrap().into_iter().map(|vsms| vsms.to_acrobits_reply()).collect::<Result<_, _>>()?
    }

    #[throws(Error)]
    pub async fn set_sms_callback(&self, opt: &Opt) {
        let url = opt.notify_url();

        let _: Value = self
            .request(hashmap! {
              "method" => "setSMS",
              "enable" => "1",
              "url_callback_enable" => "1",
              "url_callback" => &url,
              "url_callback_retry" => "1"
            })
            .await?;
    }
}

#[derive(Deserialize, Debug)]
struct VoipSendSMSResponse {
    status: String,
    sms: i64,
}

#[derive(Deserialize, Debug)]
struct VoipGetSMSResponse {
    status: String,
    sms: Option<Vec<VoipSMS>>,
}

#[derive(Deserialize, Debug)]
struct VoipSMS {
    id: String,
    #[serde(deserialize_with = "deserialize_voip_datetime")]
    date: DateTime<Utc>,
    r#type: String,
    did: String,
    contact: String,
    message: String,
}

#[derive(Serialize, Debug)]
pub struct AcrobitsSMS {
    pub sms_id: String,
    pub sending_date: DateTime<Utc>,
    #[serde(skip_serializing_if = "std::option::Option::is_none")]
    pub sender: Option<String>,
    #[serde(skip_serializing_if = "std::option::Option::is_none")]
    pub recipient: Option<String>,
    pub sms_text: String,
}

impl VoipSMS {
    #[throws(Error)]
    pub fn to_acrobits_reply(&self) -> AcrobitsSMS {
        let mut ret = AcrobitsSMS {
            sms_id: self.id.clone(),
            sending_date: self.date,
            sender: None,
            recipient: None,
            sms_text: self.message.clone(),
        };

        match self.r#type.as_str() {
            "0" => {
                // sent
                ret.recipient = Some(self.contact.clone());
            }
            "1" => {
                // received
                ret.sender = Some(self.contact.clone());
            }
            _ => unreachable!(),
        }

        ret
    }
}

pub fn deserialize_voip_datetime<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let ndt = NaiveDateTime::parse_from_str(&s, "%F %T").map_err(serde::de::Error::custom)?;
    Ok(DateTime::<Utc>::from_utc(ndt, Utc))
}

fn is_dst() -> bool {
    let now = Utc::now();
    let mut diff = now.with_timezone(&Pacific).hour() as i32 - now.hour() as i32;
    if diff > 0 {
        diff = diff - 24;
    }

    diff == -7
}

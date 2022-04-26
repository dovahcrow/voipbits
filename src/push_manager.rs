use crate::errors::VoipBitsError;
use anyhow::Error;
use aws_sdk_dynamodb::{model::AttributeValue, Client};
use fehler::throws;

pub struct PushManager {
    client: Client,
}

impl PushManager {
    pub async fn new() -> PushManager {
        let shared_config = aws_config::load_from_env().await;
        let client = Client::new(&shared_config);
        PushManager { client }
    }

    #[throws(Error)]
    pub async fn save_token(&self, did: &str, appid: &str, push_token: &str, selector: &str) {
        let record = format!("{}\\{}\\{}", appid, push_token, selector);

        self.client
            .update_item()
            .table_name("voipbits-push-tokens")
            .key("did", AttributeValue::S(did.into()))
            .update_expression("ADD tokens :tokens")
            .expression_attribute_values(":tokens", AttributeValue::Ss(vec![record]))
            .send()
            .await?;
    }

    #[throws(Error)]
    pub async fn get_tokens(&self, did: &str) -> Vec<(String, String, String)> {
        let resp = self
            .client
            .get_item()
            .table_name("voipbits-push-tokens")
            .key("did", AttributeValue::S(did.into()))
            .send()
            .await?;

        let mut record = resp
            .item
            .ok_or(VoipBitsError::NoPushTokenAvailable(did.into()))?;
        let tokens = record
            .remove("tokens")
            .ok_or(VoipBitsError::NoPushTokenAvailable(did.into()))?;
        let tokens = tokens
            .as_ss()
            .map_err(|_| VoipBitsError::NoPushTokenAvailable(did.into()))?;
        let mut rets = vec![];
        for token in tokens {
            let token: Vec<&str> = token.split("\\").collect();
            match token.as_slice() {
                [appid, push_token, selector] => rets.push((
                    appid.to_string(),
                    push_token.to_string(),
                    selector.to_string(),
                )),
                _ => unreachable!(),
            }
        }

        rets
    }

    #[throws(Error)]
    pub async fn remove_tokens(&self, did: &str, tokens: &[(String, String, String)]) {
        let records: Vec<_> = tokens
            .into_iter()
            .map(|(a, b, c)| format!("{}\\{}\\{}", a, b, c))
            .collect();
        if records.len() == 0 {
            return;
        }

        self.client
            .update_item()
            .table_name("voipbits-push-tokens")
            .key("did", AttributeValue::S(did.into()))
            .update_expression("DELETE tokens :tokens")
            .expression_attribute_values(":tokens", AttributeValue::Ss(records))
            .send()
            .await?;
    }
}

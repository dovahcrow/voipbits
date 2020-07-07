use crate::errors::VoipBitsError;
use failure::Error;
use fehler::throws;
use maplit::hashmap;
use rusoto_core::Region;
use rusoto_dynamodb::{AttributeValue, DynamoDb, DynamoDbClient, GetItemInput, UpdateItemInput};

pub struct PushManager {
    client: DynamoDbClient,
}

impl PushManager {
    pub fn new() -> PushManager {
        PushManager {
            client: DynamoDbClient::new(Region::UsWest2),
        }
    }

    #[throws(Error)]
    pub async fn save_token(&self, did: &str, appid: &str, push_token: &str, selector: &str) {
        let record = format!("{}\\{}\\{}", appid, push_token, selector);

        let req = UpdateItemInput {
            table_name: "voipbits-push-tokens".into(),
            key: hashmap! { "did".into() => AttributeValue { s: Some(did.into()), ..Default::default() } },
            update_expression: Some("ADD tokens :tokens".into()),
            expression_attribute_values: Some(hashmap! {
                ":tokens".into() => AttributeValue {
                    ss: Some(vec![record]),
                    ..Default::default()
                }
            }),
            ..Default::default()
        };

        self.client.update_item(req).await?;
    }

    #[throws(Error)]
    pub async fn get_tokens(&self, did: &str) -> Vec<(String, String, String)> {
        let req = GetItemInput {
            table_name: "voipbits-push-tokens".into(),
            key: hashmap! { "did".into() => AttributeValue { s: Some(did.into()), ..Default::default() } },
            ..Default::default()
        };

        let resp = self.client.get_item(req).await?;

        let mut record = resp.item.ok_or(VoipBitsError::NoPushTokenAvailable(did.into()))?;
        let tokens = record.remove("tokens").ok_or(VoipBitsError::NoPushTokenAvailable(did.into()))?;
        let tokens = tokens.ss.ok_or(VoipBitsError::NoPushTokenAvailable(did.into()))?;
        let mut rets = vec![];
        for token in tokens {
            let token: Vec<&str> = token.split("\\").collect();
            match token.as_slice() {
                [appid, push_token, selector] => rets.push((appid.to_string(), push_token.to_string(), selector.to_string())),
                _ => unreachable!(),
            }
        }

        rets
    }

    #[throws(Error)]
    pub async fn remove_tokens(&self, did: &str, tokens: &[(String, String, String)]) {
        let records: Vec<_> = tokens.into_iter().map(|(a, b, c)| format!("{}\\{}\\{}", a, b, c)).collect();
        if records.len() == 0 {
            return;
        }

        let req = UpdateItemInput {
            table_name: "voipbits-push-tokens".into(),
            key: hashmap! { "did".into() => AttributeValue { s: Some(did.into()), ..Default::default() } },
            update_expression: Some("DELETE tokens :tokens".into()),
            expression_attribute_values: Some(hashmap! {
                ":tokens".into() => AttributeValue {
                    ss: Some(records),
                    ..Default::default()
                }
            }),
            ..Default::default()
        };

        self.client.update_item(req).await?;
    }
}

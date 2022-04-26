use anyhow::Error;
use fehler::throws;
use maplit::hashmap;
use reqwest::Client;

pub struct Acrobits {
    client: Client,
}

impl Acrobits {
    pub fn new() -> Acrobits {
        Acrobits {
            client: Client::new(),
        }
    }

    #[throws(Error)]
    #[tracing::instrument(skip(self))]
    pub async fn notify(
        &self,
        appid: &str,
        device_token: &str,
        selector: &str,
        from: &str,
        message: &str,
    ) {
        self.client
            .post("https://pnm.cloudsoftphone.com/pnm2/send")
            .json(&hashmap! {
                "verb" => "NotifyTextMessage",
                // "Id" => "" // Voipms actually doesn't give us the messageid when notify us
                "Selector" => selector,
                "Badge" => "1",
                "UserName" => from,
                "Message" => message,
                "AppId" => appid,
                "DeviceToken" => device_token,
            })
            .send()
            .await?;
    }
}

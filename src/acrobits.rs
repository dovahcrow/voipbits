use failure::Error;
use fehler::throws;
use maplit::hashmap;
use reqwest::Client;

pub struct Acrobits {
    client: Client,
}

impl Acrobits {
    pub fn new() -> Acrobits {
        Acrobits { client: Client::new() }
    }

    #[throws(Error)]
    pub async fn notify(&self, appid: &str, device_token: &str, selector: &str, _from: &str, _message: &str) {
        self.client
            .post("https://pnm.cloudsoftphone.com/pnm2")
            .form(&hashmap! {
                "verb" => "NotifyTextMessage",
                // "Id" => "" // Voipms actually doesn't give us the messageid when notify us
                "Selector" => selector,
                "Badge" => "1",
                "UserName" => _from,
                "Message" => _message,
                "AppId" => appid,
                "DeviceToken" => device_token,
            })
            .send()
            .await?;
    }
}

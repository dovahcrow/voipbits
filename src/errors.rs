use thiserror::Error;

#[derive(Error, Debug)]
pub enum VoipBitsError {
    #[error("Empty message")]
    EmptyMessage,
    #[error("Invalid number: {0}")]
    InvalidNumber(String),
    #[error("No such SMS with id {0}")]
    NoSuchSMS(String),
    #[error("No push token available for {0}")]
    NoPushTokenAvailable(String),
}

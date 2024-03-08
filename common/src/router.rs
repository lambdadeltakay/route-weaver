use serde::{Deserialize, Serialize};
use sha2::Digest;
use sha2::Sha256;
use zeroize::ZeroizeOnDrop;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, ZeroizeOnDrop)]
pub struct ApplicationId(pub [u8; 32]);

impl From<&str> for ApplicationId {
    fn from(value: &str) -> Self {
        ApplicationId(Sha256::digest(value.as_bytes()).into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RouterBoundMessage {
    RegisterApplication { application_id: ApplicationId },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientBoundMessage {
    ApplicationRegistered { application_id: ApplicationId },
}

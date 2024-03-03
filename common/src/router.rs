use serde::{Deserialize, Serialize};

use crate::message::ApplicationId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RouterBoundMessage {
    RegisterApplication { application_id: ApplicationId },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientBoundMessage {
    ApplicationRegistered { application_id: ApplicationId },
}

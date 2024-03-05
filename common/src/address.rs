use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::{fmt::Display, str::FromStr};

use crate::error::RouteWeaverError;

#[derive(Clone, Debug, Hash, PartialEq, Eq, SerializeDisplay, DeserializeFromStr)]
pub struct TransportAddress {
    pub address_type: String,
    pub protocol: String,
    pub data: String,
    pub port: Option<u16>,
}

impl TransportAddress {
    // For comparing addresses so we don't connect too many times
    pub fn without_port(&self) -> Self {
        Self {
            address_type: self.address_type.clone(),
            protocol: self.protocol.clone(),
            data: self.data.clone(),
            port: None,
        }
    }
}

impl FromStr for TransportAddress {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('/');

        parts.next().ok_or(RouteWeaverError::AddressFailedToParse)?;

        let address_type = parts
            .next()
            .ok_or(RouteWeaverError::AddressFailedToParse)?
            .to_string();
        let data = parts
            .next()
            .ok_or(RouteWeaverError::AddressFailedToParse)?
            .to_string();
        let protocol = parts
            .next()
            .ok_or(RouteWeaverError::AddressFailedToParse)?
            .to_string();

        let port = if let Some(port) = parts.next() {
            Some(
                port.parse()
                    .map_err(|_| RouteWeaverError::AddressFailedToParse)?,
            )
        } else {
            None
        };

        Ok(TransportAddress {
            address_type,
            protocol,
            data,
            port,
        })
    }
}

impl Display for TransportAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(port) = self.port {
            write!(
                f,
                "/{}/{}/{}/{}",
                self.address_type, self.data, self.protocol, port
            )
        } else {
            write!(f, "/{}/{}/{}", self.address_type, self.data, self.protocol)
        }
    }
}

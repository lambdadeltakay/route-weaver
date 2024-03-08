use arrayvec::ArrayString;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::{fmt::Display, str::FromStr};

use crate::error::RouteWeaverError;

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportAddress {
    pub address_type: ArrayString<8>,
    pub protocol: ArrayString<8>,
    pub data: ArrayString<128>,
    pub port: Option<u16>,
}

impl TransportAddress {
    // For comparing addresses so we don't connect too many times
    pub fn without_port(&self) -> Self {
        Self {
            address_type: self.address_type,
            protocol: self.protocol,
            data: self.data,
            port: None,
        }
    }
}

impl FromStr for TransportAddress {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('/');

        parts.next().ok_or(RouteWeaverError::AddressFailedToParse)?;

        let address_type =
            ArrayString::from(parts.next().ok_or(RouteWeaverError::AddressFailedToParse)?)
                .map_err(|_| RouteWeaverError::AddressFailedToParse)?;
        let data = ArrayString::from(parts.next().ok_or(RouteWeaverError::AddressFailedToParse)?)
            .map_err(|_| RouteWeaverError::AddressFailedToParse)?;
        let protocol =
            ArrayString::from(parts.next().ok_or(RouteWeaverError::AddressFailedToParse)?)
                .map_err(|_| RouteWeaverError::AddressFailedToParse)?;

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

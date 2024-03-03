use std::{fmt::Display, str::FromStr};
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[derive(Clone, Debug, Hash, PartialEq, Eq, SerializeDisplay, DeserializeFromStr)]
pub struct TransportAddress {
    pub address_type: String,
    pub protocol: String,
    pub data: String,
    pub port: Option<u16>,
}

impl FromStr for TransportAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('/');

        let address_type = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid address"))?
            .to_string();
        let protocol = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid address"))?
            .to_string();
        let data = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid address"))?
            .to_string();

        let port = if let Some(port) = parts.next() {
            Some(port.parse()?)
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

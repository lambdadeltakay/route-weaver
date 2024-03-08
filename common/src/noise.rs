use std::{fmt::Display, str::FromStr};

use data_encoding::HEXLOWER_PERMISSIVE;

use serde::{Deserialize, Serialize};

use zeroize::ZeroizeOnDrop;

use crate::error::RouteWeaverError;

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl FromStr for PublicKey {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PublicKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?
                .try_into()
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?,
        ))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct PrivateKey(pub [u8; 32]);

impl Display for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl FromStr for PrivateKey {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PrivateKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?
                .try_into()
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?,
        ))
    }
}

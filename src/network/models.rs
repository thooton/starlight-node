use serde::{Deserialize, Serialize};

use crate::{
    error, keys::{Hash, HashBuilder, Private, Public, Signature}, protocol::{Amount, Slot}, util::{self, Error, Version}
};

use super::{center_map::CenterMapValue, endpoint::Endpoint, shred::Shred};

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Peer {
    pub weight: Amount,
    pub last_contact: Slot,
    pub logical: Endpoint,
    pub version: Version,
}
impl CenterMapValue<Amount> for Peer {
    fn priority(&self) -> Amount {
        self.weight
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TelemetryMsg {
    pub from: Public,
    pub signature: Signature,
    pub slot: Slot,
    pub ep: Endpoint,
    pub version: Version,
}
impl TelemetryMsg {
    fn hash_pieces(slot: Slot, logical: Endpoint, version: Version) -> Hash {
        let mut buf = [0u8; 20];
        buf[0..8].copy_from_slice(&slot.to_bytes());
        buf[8..14].copy_from_slice(&logical.to_bytes());
        buf[14..20].copy_from_slice(&version.to_bytes());
        Hash::digest(&buf)
    }
    pub fn sign_new(private: Private, slot: Slot, ep: Endpoint, version: Version) -> Self {
        let hash = Self::hash_pieces(slot, ep, version);
        let signature = private.sign(&hash);
        Self {
            from: private.to_public(),
            signature,
            slot,
            ep,
            version,
        }
    }
    pub fn hash(&self) -> Hash {
        Self::hash_pieces(self.slot, self.ep, self.version)
    }
    pub fn verify(&self) -> Result<(), Error> {
        let hash = self.hash();
        self.from.verify(&hash, &self.signature)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ShredMsg {
    pub from: Public,
    pub signature: Signature,
    pub slot: Slot,
    pub shred: Shred,
}
impl ShredMsg {
    pub fn hash(&self) -> Hash {
        let mut hb = HashBuilder::new();
        hb.update(&self.slot.to_bytes());
        self.shred.hash_into(&mut hb);
        hb.finish()
    }
    pub fn verify(&self) -> Result<(), Error> {
        let hash = self.hash();
        self.from.verify(&hash, &self.signature)
    }
}

const MAGIC_NUMBER: [u8; 8] = [0x3f, 0xd1, 0x0f, 0xe2, 0x5e, 0x76, 0xfa, 0xe6];

#[derive(Serialize, Deserialize, Clone)]
pub enum Msg {
    Tel(Box<TelemetryMsg>),
    Shred(Box<ShredMsg>),
}
impl Msg {
    pub fn serialize(&self, mtu: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(mtu);
        bytes.extend_from_slice(&MAGIC_NUMBER);
        util::serialize_into(&mut bytes, self);
        bytes
    }
    pub fn deserialize(bytes: &[u8], mtu: usize) -> Result<Self, Error> {
        if bytes.len() < 8 {
            return Err(error!("message too small"));
        }
        if bytes[0..8] != MAGIC_NUMBER {
            return Err(error!("wrong magic number"));
        }
        if bytes.len() > mtu {
            return Err(error!("message too large"));
        }
        util::deserialize(&bytes[8..]).or_else(|_| {
            return Err(error!("invalid message"));
        })
    }
    pub fn verify(&self) -> Result<(), Error> {
        match self {
            Msg::Tel(t) => t.verify(),
            Msg::Shred(s) => s.verify(),
        }
    }
}

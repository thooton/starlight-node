// Derived from the keys module of github.com/feeless/feeless@978eba7.
use super::signature::Signature;
use super::Hash;
use crate::bail;
use crate::error;
use crate::hexify;
use crate::util::Error;
use blake2b_simd::Params;
use ed25519_dalek_blake2_feeless::PublicKey;
use ed25519_dalek_blake2_feeless::Verifier;
use primitive_types::U512;
use serde::Serialize;
use serde::{Deserialize, Deserializer, Serializer};

/// 256 bit public key which can be converted into an [Address](crate::Address) or verify a [Signature](crate::Signature).
#[derive(Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(align(8))]
pub struct Public([u8; 32]);

hexify!(Public, "public key");

fn decode_to_u512(s: &str) -> Result<U512, Error> {
    if !is_valid(s) {
        bail!("invalid account string");
    }

    let mut number = U512::default();
    for character in s.chars().skip(4) {
        match decode_byte(character) {
            Some(byte) => {
                number <<= 5;
                number = number + byte;
            }
            None => bail!("invalid hex string"),
        }
    }
    Ok(number)
}

fn is_valid(s: &str) -> bool {
    s.starts_with("slt_")
        && s.chars().count() == 64
        && matches!(s.chars().nth(4), Some('1') | Some('3'))
}

fn checksum_bytes(number: U512) -> [u8; 5] {
    [
        number.byte(0),
        number.byte(1),
        number.byte(2),
        number.byte(3),
        number.byte(4),
    ]
}

fn account_bytes(number: U512) -> [u8; 32] {
    let mut bytes_512 = [0u8; 64];
    (number >> 40).to_big_endian(&mut bytes_512);
    let mut bytes_256 = [0u8; 32];
    bytes_256.copy_from_slice(&bytes_512[32..]);
    bytes_256
}

fn decode_byte(character: char) -> Option<u8> {
    if character.is_ascii() {
        let character = character as u8;
        if (0x30..0x80).contains(&character) {
            let byte: u8 = account_decode(character);
            if byte != b'~' {
                return Some(byte);
            }
        }
    }

    None
}

const ACCOUNT_LOOKUP: &[char] = &[
    '1', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k',
    'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'w', 'x', 'y', 'z',
];

const ACCOUNT_REVERSE: &[char] = &[
    '~', '0', '~', '1', '2', '3', '4', '5', '6', '7', '~', '~', '~', '~', '~', '~', '~', '~', '~',
    '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~',
    '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '~', '8', '9', ':', ';', '<', '=', '>', '?',
    '@', 'A', 'B', '~', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', '~', 'L', 'M', 'N', 'O', '~',
    '~', '~', '~', '~',
];

fn account_encode(value: u8) -> char {
    ACCOUNT_LOOKUP[value as usize]
}

fn account_decode(value: u8) -> u8 {
    let mut result = ACCOUNT_REVERSE[(value - 0x30) as usize] as u8;
    if result != b'~' {
        result -= 0x30;
    }
    result
}

impl Public {
    /// Convert the public key to an address string
    pub fn to_address(&self) -> String {
        let mut number = U512::from_big_endian(&self.0);
        let check = U512::from_little_endian(&self.checksum());
        number <<= 40;
        number |= check;

        let mut result = String::with_capacity(65);

        for _i in 0..60 {
            let r = number.byte(0) & 0x1f_u8;
            number >>= 5;
            result.push(account_encode(r));
        }
        result.push_str("_tls");
        result.chars().rev().collect()
    }

    /// Create a public key from an address string
    pub fn from_address(address: &str) -> Result<Self, Error> {
        let number = decode_to_u512(address)?;
        let public = Public(account_bytes(number));
        if public.checksum() != checksum_bytes(number) {
            bail!("invalid checksum");
        }
        Ok(public)
    }
}

#[static_init::dynamic]
static PARAMS: Params = {
    let mut params = Params::new();
    params.hash_length(5);
    params
};

impl Public {
    pub const LEN: usize = 32;
    const ADDRESS_CHECKSUM_LEN: usize = 5;

    fn dalek_key(&self) -> Result<PublicKey, Error> {
        Ok(PublicKey::from_bytes(&self.0).map_err(|e| error!("Converting to PublicKey: {}", e))?)
    }

    fn checksum(&self) -> [u8; 5] {
        PARAMS.hash(&self.0).as_bytes().try_into().unwrap()
    }

    pub fn zero() -> Self {
        Self([0u8; 32])
    }

    pub fn verify(&self, hash: &Hash, signature: &Signature) -> Result<(), Error> {
        let dalek_key = self.dalek_key().or_else(|_| {
            return Err(error!("invalid public key"));
        })?;
        let signature_internal = signature.internal().or_else(|_| {
            return Err(error!("invalid signature"));
        })?;
        dalek_key
            .verify(hash.as_bytes(), &signature_internal)
            .or_else(|_| {
                return Err(error!("verification failed"));
            })
    }
}

impl From<PublicKey> for Public {
    fn from(v: PublicKey) -> Self {
        Self(*v.as_bytes())
    }
}

/// A serde serializer that converts to an address instead of public key hexes.
///
/// Use with #[serde(serialize_with = "to_address")] on the field that needs it.
pub fn to_address<S>(public: &Public, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(public.to_address().to_string().as_str())
}

pub fn from_address<'de, D>(deserializer: D) -> Result<Public, <D as Deserializer<'de>>::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    Ok(Public::from_address(s).map_err(serde::de::Error::custom)?)
}

#[cfg(test)]
mod tests {
    use super::Public;
    use crate::keys::private::Private;
    use std::str::FromStr;

    /// Example private -> public conversion:
    /// https://docs.nano.org/protocol-design/signing-hashing-and-key-derivation/#signing-algorithm-ed25519
    #[test]
    fn empty_private_to_public() {
        let private = Private::zero();
        let public = private.to_public();
        // If the result is...
        // 3B6A27BCCEB6A42D62A3A8D02A6F0D73653215771DE243A63AC048A18B59DA29
        // ...it means we're using sha512 instead of blake2b for the hasher.
        assert_eq!(
            public.to_string(),
            "19D3D919475DEED4696B5D13018151D1AF88B2BD3BCFF048B45031C1F36D1858"
        )
    }

    #[test]
    fn hex() {
        let s = "19D3D919475DEED4696B5D13018151D1AF88B2BD3BCFF048B45031C1F36D1858";
        assert_eq!(s, &Public::from_str(&s).unwrap().as_hex());
    }
}

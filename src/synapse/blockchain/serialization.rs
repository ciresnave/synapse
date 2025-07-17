use bincode::{Encode, Decode, BorrowDecode};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateTimeWrapper(pub DateTime<Utc>);

impl Default for DateTimeWrapper {
    fn default() -> Self {
        DateTimeWrapper(Utc::now())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UuidWrapper(pub Uuid);

impl fmt::Display for UuidWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl DateTimeWrapper {
    pub fn new(dt: DateTime<Utc>) -> Self {
        DateTimeWrapper(dt)
    }

    pub fn into_inner(self) -> DateTime<Utc> {
        self.0
    }

    // Manual serialization methods that don't rely on derive macros
    pub fn to_bincode(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        let timestamp = self.0.timestamp();
        let nanos = self.0.timestamp_subsec_nanos();
        bincode::encode_to_vec((timestamp, nanos), bincode::config::standard())
    }

    pub fn from_bincode(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        let (timestamp, nanos): (i64, u32) = bincode::decode_from_slice(bytes, bincode::config::standard())?.0;
        let dt = DateTime::<Utc>::from_timestamp(timestamp, nanos)
            .ok_or_else(|| bincode::error::DecodeError::OtherString(
                "Invalid DateTime values".to_string()
            ))?;
        Ok(DateTimeWrapper(dt))
    }
}

impl Encode for DateTimeWrapper {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        let timestamp = self.0.timestamp();
        let nanos = self.0.timestamp_subsec_nanos();
        timestamp.encode(encoder)?;
        nanos.encode(encoder)?;
        Ok(())
    }
}

impl<Context> Decode<Context> for DateTimeWrapper {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let timestamp = i64::decode(decoder)?;
        let nanos = u32::decode(decoder)?;
        
        let dt = DateTime::<Utc>::from_timestamp(timestamp, nanos)
            .ok_or_else(|| bincode::error::DecodeError::OtherString(
                "Invalid DateTime values".to_string()
            ))?;
            
        Ok(DateTimeWrapper(dt))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for DateTimeWrapper {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let timestamp = i64::borrow_decode(decoder)?;
        let nanos = u32::borrow_decode(decoder)?;
        
        let dt = DateTime::<Utc>::from_timestamp(timestamp, nanos)
            .ok_or_else(|| bincode::error::DecodeError::OtherString(
                "Invalid DateTime values".to_string()
            ))?;
            
        Ok(DateTimeWrapper(dt))
    }
}

impl UuidWrapper {
    pub fn new(uuid: Uuid) -> Self {
        UuidWrapper(uuid)
    }

    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

impl Encode for UuidWrapper {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.0.as_bytes().encode(encoder)
    }
}

impl<Context> Decode<Context> for UuidWrapper {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let bytes: [u8; 16] = Decode::decode(decoder)?;
        Ok(UuidWrapper(Uuid::from_bytes(bytes)))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for UuidWrapper {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let bytes: [u8; 16] = BorrowDecode::borrow_decode(decoder)?;
        Ok(UuidWrapper(Uuid::from_bytes(bytes)))
    }
}

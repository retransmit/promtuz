use std::io;

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::io::AsyncReadExt;

/// Hard cap on one framed packet. `unpack` allocates a buffer of the
/// peer-declared length before reading, so this bounds a hostile length (an
/// unbounded one is an OOM). Also the effective max message + inline-media
/// size; larger media rides the out-of-band blob path.
///
/// ponytail: 1 MiB. Raise for bigger inline media, but the relay queues up to
/// MAX_QUEUED_PER_RECIPIENT copies at K homes, so disk scales with this.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

#[derive(Debug, Error)]
pub enum PackError {
    #[error("failed to serialize: {0}")]
    SerFailed(postcard::Error),
    #[error("packet too large: {0} bytes exceeds MAX_FRAME_BYTES")]
    FrameTooLarge(usize),
}

#[derive(Debug, Error)]
pub enum UnpackError {
    #[error("failed to read: {0}")]
    ReadFailed(io::Error),
    #[error("failed to deserialize: {0}")]
    DeserFailed(postcard::Error),
    #[error("frame too large: {0} bytes exceeds MAX_FRAME_BYTES")]
    FrameTooLarge(usize),
}

/// Decides which structs and enums can be packed for network transmission
///
/// Only use for data that is sent over network and not locally
pub trait Packer {
    fn ser(&self) -> Result<Vec<u8>, PackError>;
    fn pack(&self) -> Result<Vec<u8>, PackError>;
}

impl<T> Packer for T
where
    T: Serialize,
{
    #[inline]
    fn ser(&self) -> Result<Vec<u8>, PackError> {
        postcard::to_allocvec(self).map_err(PackError::SerFailed)
    }

    /// Frames bytes after serializing as ready to transmit Packet
    #[inline]
    fn pack(&self) -> Result<Vec<u8>, PackError> {
        let packet = self.ser()?;
        if packet.len() > MAX_FRAME_BYTES {
            return Err(PackError::FrameTooLarge(packet.len()));
        }
        let len = packet.len() as u32;
        let mut out = Vec::with_capacity(4 + packet.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&packet);
        Ok(out)
    }
}

#[async_trait]
pub trait Unpacker: Sized {
    fn deser(bytes: &[u8]) -> Result<Self, UnpackError>;

    async fn unpack<R>(rx: &mut R) -> Result<Self, UnpackError>
    where
        R: AsyncReadExt + Unpin + Send;
}

#[async_trait]
impl<T> Unpacker for T
where
    T: DeserializeOwned,
{
    #[inline]
    fn deser(bytes: &[u8]) -> Result<Self, UnpackError> {
        // let cursor = Cursor::new(bytes);
        // Ok(ciborium::de::from_reader(cursor)?)

        postcard::from_bytes(bytes).map_err(UnpackError::DeserFailed)
    }

    async fn unpack<R>(rx: &mut R) -> Result<Self, UnpackError>
    where
        R: AsyncReadExt + Unpin + Send,
    {
        unpack(rx).await
    }
}

#[inline(always)]
pub async fn unpack<T: DeserializeOwned, R: AsyncReadExt + Unpin + Send>(
    rx: &mut R,
) -> Result<T, UnpackError> {
    let frame_size = rx.read_u32().await.map_err(UnpackError::ReadFailed)? as usize;
    if frame_size > MAX_FRAME_BYTES {
        return Err(UnpackError::FrameTooLarge(frame_size));
    }
    let mut frame = vec![0u8; frame_size];
    rx.read_exact(&mut frame).await.map_err(UnpackError::ReadFailed)?;

    T::deser(&frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frame_roundtrips_through_u32_prefix() {
        let msg: Vec<u8> = (0..300).map(|i| i as u8).collect(); // > old u8/u16-fiddly sizes
        let framed = msg.pack().expect("pack");
        let body_len = msg.ser().unwrap().len();
        assert_eq!(&framed[..4], &(body_len as u32).to_be_bytes(), "4-byte BE length prefix");
        let mut rx: &[u8] = &framed;
        let out: Vec<u8> = unpack(&mut rx).await.expect("unpack");
        assert_eq!(out, msg);
    }

    #[test]
    fn pack_rejects_oversize() {
        let big = vec![0u8; MAX_FRAME_BYTES + 1];
        assert!(matches!(big.pack(), Err(PackError::FrameTooLarge(_))));
    }

    #[tokio::test]
    async fn unpack_rejects_oversize_length_before_reading_body() {
        // Only the 4-byte length is present (no body) — proves we reject on the
        // declared length before allocating/reading, the OOM guard.
        let framed = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes();
        let mut rx: &[u8] = &framed;
        let r: Result<Vec<u8>, _> = unpack(&mut rx).await;
        assert!(matches!(r, Err(UnpackError::FrameTooLarge(_))));
    }
}

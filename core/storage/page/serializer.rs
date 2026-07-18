/// Variable-length unsigned integer encoding (LEB128) and raw byte serialization.
///
/// # VarInt encoding
///
/// Small values (0-127) take 1 byte, larger values take up to 10 bytes
/// (u64::MAX requires 10 bytes since 9*7=63 bits + 1 final byte).
/// Each byte encodes 7 bits of data. The high bit (0x80) indicates
/// "more bytes follow". The final byte has the high bit cleared.
///
/// ```
/// use hive_core::storage::page::serializer::{var_int_write, var_int_read, var_int_size};
/// let mut buf = [0u8; 10];
///
/// // Write a varint
/// let n = var_int_write(&mut buf, 300);
/// assert_eq!(n, 2);
/// assert_eq!(var_int_size(300), 2);
///
/// // Read it back
/// let (val, read) = var_int_read(&buf).unwrap();
/// assert_eq!(val, 300);
/// assert_eq!(read, 2);
/// ```
///
/// # Fixed-width helpers
///
/// ```
/// use hive_core::storage::page::serializer::{put_u16_le, get_u16_le, put_u32_le, get_u32_le};
/// let mut buf = [0u8; 8];
/// put_u16_le(&mut buf, 0, 0x012C);
/// assert_eq!(get_u16_le(&buf, 0), 300);
/// put_u32_le(&mut buf, 4, 0xDEADBEEF);
/// assert_eq!(get_u32_le(&buf, 4), 0xDEADBEEF);
/// ```
use crate::errors::DbError;

pub const MAX_VARINT_BYTES: usize = 10;

/// Returns how many bytes are needed to encode a `u64` as a varint.
pub fn var_int_size(value: u64) -> usize {
    match value {
        0..=127 => 1,
        128..=16383 => 2,
        16384..=2097151 => 3,
        2097152..=268435455 => 4,
        268435456..=34359738367 => 5,
        34359738368..=4398046511103 => 6,
        4398046511104..=562949953421311 => 7,
        562949953421312..=72057594037927935 => 8,
        72057594037927936..=9223372036854775807 => 9,
        _ => 10,
    }
}

/// Encodes a `u64` into the buffer using little-endian base-128 varint format.
pub fn var_int_write(buf: &mut [u8], value: u64) -> usize {
    let mut v = value;
    let mut i = 0;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
            buf[i] = byte;
            i += 1;
        } else {
            buf[i] = byte;
            i += 1;
            break;
        }
    }
    i
}
/// Decodes a varint from the buffer and returns the value plus bytes consumed.
pub fn var_int_read(buf: &[u8]) -> Result<(u64, usize), DbError> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for (i, &byte) in buf.iter().enumerate() {
        if i >= MAX_VARINT_BYTES {
            return Err(DbError::ReadError);
        }
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err(DbError::ReadError);
        }
    }
    Err(DbError::ReadError)
}

/// Writes one byte at an offset in a raw page/record buffer.
#[inline]
pub fn put_u8(buf: &mut [u8], offset: usize, value: u8) {
    buf[offset] = value;
}

/// Reads one byte at an offset in a raw page/record buffer.
#[inline]
pub fn get_u8(buf: &[u8], offset: usize) -> u8 {
    buf[offset]
}

/// Writes a little-endian `u16` at an offset in a raw buffer.
#[inline]
pub fn put_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Reads a little-endian `u16` at an offset in a raw buffer.
#[inline]
pub fn get_u16_le(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

/// Writes a little-endian `u32` at an offset in a raw buffer.
#[inline]
pub fn put_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Reads a little-endian `u32` at an offset in a raw buffer.
#[inline]
pub fn get_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

/// Writes a little-endian `u64` at an offset in a raw buffer.
#[inline]
pub fn put_u64_le(buf: &mut [u8], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Reads a little-endian `u64` at an offset in a raw buffer.
#[inline]
pub fn get_u64_le(buf: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
        buf[offset + 4],
        buf[offset + 5],
        buf[offset + 6],
        buf[offset + 7],
    ])
}

/// Computes a CRC32 checksum over the selected byte range.
pub fn compute_checksum(buf: &[u8], start: usize, end: usize) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&buf[start..end]);
    hasher.finalize()
}

// Tests for LEB128 varint encoding and binary read/write helpers.
use crate::storage::page::{serializer, serializer::MAX_VARINT_BYTES};

#[test]
fn write_then_read_varint_small_values() {
    let mut buf = [0u8; MAX_VARINT_BYTES];
    let test_vals = [0u64, 1, 42, 127, 128, 255, 300, 16383];

    for &val in &test_vals {
        let written = serializer::var_int_write(&mut buf, val);
        let (decoded, read) = serializer::var_int_read(&buf).unwrap();
        assert_eq!(decoded, val);
        assert_eq!(read, written);
    }
}

#[test]
fn write_then_read_varint_medium_values() {
    let mut buf = [0u8; MAX_VARINT_BYTES];
    let test_vals = [16384u64, 100_000, 1_000_000, 10_000_000, u32::MAX as u64];

    for &val in &test_vals {
        let written = serializer::var_int_write(&mut buf, val);
        let (decoded, read) = serializer::var_int_read(&buf).unwrap();
        assert_eq!(decoded, val);
        assert_eq!(read, written);
    }
}

#[test]
fn write_then_read_varint_large_values() {
    let mut buf = [0u8; MAX_VARINT_BYTES];
    let test_vals = [
        1_000_000_000u64,
        100_000_000_000,
        u64::MAX / 2,
        u64::MAX - 1,
        u64::MAX,
    ];

    for &val in &test_vals {
        let written = serializer::var_int_write(&mut buf, val);
        let (decoded, read) = serializer::var_int_read(&buf).unwrap();
        assert_eq!(decoded, val, "failed for {}", val);
        assert_eq!(read, written);
    }
}

#[test]
fn varint_size_matches_write() {
    let mut buf = [0u8; MAX_VARINT_BYTES];
    let test_vals = [
        0u64,
        127,
        128,
        16383,
        16384,
        1_000_000,
        u32::MAX as u64,
        u64::MAX,
    ];

    for &val in &test_vals {
        let written = serializer::var_int_write(&mut buf, val);
        assert_eq!(serializer::var_int_size(val), written);
    }
}

#[test]
fn read_varint_truncated_input_returns_error() {
    let buf = [0x80u8; 1];
    assert!(serializer::var_int_read(&buf).is_err());
}

#[test]
fn u16_le_roundtrip() {
    let mut buf = [0u8; 2];
    let vals = [0u16, 1, 255, 256, 0xABCD, u16::MAX];
    for &val in &vals {
        serializer::put_u16_le(&mut buf, 0, val);
        assert_eq!(serializer::get_u16_le(&buf, 0), val);
    }
}

#[test]
fn u32_le_roundtrip() {
    let mut buf = [0u8; 4];
    let vals = [0u32, 1, 0xDEAD_BEEF, u32::MAX];
    for &val in &vals {
        serializer::put_u32_le(&mut buf, 0, val);
        assert_eq!(serializer::get_u32_le(&buf, 0), val);
    }
}

#[test]
fn u64_le_roundtrip() {
    let mut buf = [0u8; 8];
    let vals = [0u64, 1, 0xCAFE_BABE_DEAD_BEEF, u64::MAX];
    for &val in &vals {
        serializer::put_u64_le(&mut buf, 0, val);
        assert_eq!(serializer::get_u64_le(&buf, 0), val);
    }
}

#[test]
fn u8_roundtrip() {
    let mut buf = [0u8; 1];
    for val in 0..=255u8 {
        serializer::put_u8(&mut buf, 0, val);
        assert_eq!(serializer::get_u8(&buf, 0), val, "failed for {}", val);
    }
}

#[test]
fn checksum_deterministic() {
    let data = b"page checksum test data";
    let cs1 = serializer::compute_checksum(data, 0, data.len());
    let cs2 = serializer::compute_checksum(data, 0, data.len());
    assert_eq!(cs1, cs2);
}

#[test]
fn checksum_differs_for_different_data() {
    let a = b"alpha";
    let b = b"beta";
    let cs_a = serializer::compute_checksum(a, 0, a.len());
    let cs_b = serializer::compute_checksum(b, 0, b.len());
    assert_ne!(cs_a, cs_b);
}

#[test]
fn checksum_differs_for_different_ranges() {
    let data = b"1234567890";
    let cs_full = serializer::compute_checksum(data, 0, data.len());
    let cs_partial = serializer::compute_checksum(data, 0, 5);
    assert_ne!(cs_full, cs_partial);
}

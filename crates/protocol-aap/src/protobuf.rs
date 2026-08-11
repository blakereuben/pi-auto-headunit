//! Shared hand-rolled protobuf wire primitives.
//!
//! `service_discovery.rs` and the channel-setup decoders/encoders planned
//! alongside `ServiceDiscoveryResponse` support all need the same small set
//! of proto2 varint/tag/length-delimited primitives. This module holds them
//! once instead of duplicating them per file. Functions are generic over
//! the caller's own error type (via [`ProtobufDecodeError`]) so each call
//! site keeps its existing, already-tested error variants and `Display`
//! text.
//!
//! Only the primitives an existing caller uses today live here.

/// Constructs the handful of decode-error outcomes every hand-rolled
/// protobuf reader in this crate can hit, in the caller's own error type.
pub(crate) trait ProtobufDecodeError {
    fn truncated() -> Self;
    fn invalid_varint() -> Self;
    fn invalid_field_number() -> Self;
    fn length_not_representable() -> Self;
    fn unsupported_wire_type(wire_type: u8) -> Self;
}

pub(crate) fn read_varint<E: ProtobufDecodeError>(
    input: &[u8],
    cursor: &mut usize,
) -> Result<u64, E> {
    let mut value = 0_u64;
    for index in 0..10 {
        let byte = *input.get(*cursor).ok_or_else(E::truncated)?;
        *cursor += 1;
        if index == 9 && byte > 1 {
            return Err(E::invalid_varint());
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(E::invalid_varint())
}

/// Decodes a zigzag-encoded `sint32`/`sint64` varint (distinct from a plain
/// `int32`/`int64`/enum varint, which sign-extends instead of zigzagging).
pub(crate) fn read_zigzag_varint<E: ProtobufDecodeError>(
    input: &[u8],
    cursor: &mut usize,
) -> Result<i64, E> {
    let encoded = read_varint::<E>(input, cursor)?;
    // `encoded >> 1` fits in 63 bits and `encoded & 1` is 0 or 1, so neither
    // u64->i64 cast can actually wrap — this is the standard zigzag decode.
    #[allow(clippy::cast_possible_wrap)]
    let value = ((encoded >> 1) as i64) ^ -((encoded & 1) as i64);
    Ok(value)
}

/// Reads a field tag (`(field_number, wire_type)`), rejecting field number
/// zero the same way every existing decode call site already does.
pub(crate) fn read_tag<E: ProtobufDecodeError>(
    input: &[u8],
    cursor: &mut usize,
) -> Result<(u32, u8), E> {
    let key = read_varint::<E>(input, cursor)?;
    let field = u32::try_from(key >> 3).map_err(|_| E::invalid_field_number())?;
    if field == 0 {
        return Err(E::invalid_field_number());
    }
    let wire_type = (key & 0x07) as u8;
    Ok((field, wire_type))
}

pub(crate) fn read_length_delimited<'a, E: ProtobufDecodeError>(
    input: &'a [u8],
    cursor: &mut usize,
) -> Result<&'a [u8], E> {
    let length = usize::try_from(read_varint::<E>(input, cursor)?)
        .map_err(|_| E::length_not_representable())?;
    let end = cursor
        .checked_add(length)
        .ok_or_else(E::length_not_representable)?;
    let value = input.get(*cursor..end).ok_or_else(E::truncated)?;
    *cursor = end;
    Ok(value)
}

pub(crate) fn skip_bytes<E: ProtobufDecodeError>(
    input: &[u8],
    cursor: &mut usize,
    count: usize,
) -> Result<(), E> {
    let end = cursor
        .checked_add(count)
        .ok_or_else(E::length_not_representable)?;
    input.get(*cursor..end).ok_or_else(E::truncated)?;
    *cursor = end;
    Ok(())
}

pub(crate) fn skip_unknown_field<E: ProtobufDecodeError>(
    input: &[u8],
    cursor: &mut usize,
    wire_type: u8,
) -> Result<(), E> {
    match wire_type {
        0 => {
            read_varint::<E>(input, cursor)?;
            Ok(())
        }
        1 => skip_bytes(input, cursor, 8),
        2 => {
            read_length_delimited::<E>(input, cursor)?;
            Ok(())
        }
        5 => skip_bytes(input, cursor, 4),
        value => Err(E::unsupported_wire_type(value)),
    }
}

fn write_varint(out: &mut Vec<u8>, value: u64) {
    let mut remaining = value;
    loop {
        let byte = (remaining & 0x7f) as u8;
        remaining >>= 7;
        if remaining == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn write_tag(out: &mut Vec<u8>, field: u32, wire_type: u8) {
    write_varint(out, (u64::from(field) << 3) | u64::from(wire_type));
}

/// Writes a proto2 `int32`/`int64` or enum field — both use the same wire
/// encoding (varint, sign-extended through `i64` for negative values; see
/// `BluetoothPairingMethod`'s `-1` in `docs/protocol/aasdk-adoption.md` for
/// a real negative enum value elsewhere in this schema, unused this pass).
pub(crate) fn write_int32_field(out: &mut Vec<u8>, field: u32, value: i32) {
    write_tag(out, field, 0);
    #[allow(clippy::cast_sign_loss)]
    write_varint(out, i64::from(value) as u64);
}

/// Writes a length-delimited field — `string`, `bytes`, or a pre-encoded
/// nested message body.
pub(crate) fn write_length_delimited_field(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    write_tag(out, field, 2);
    write_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// Writes a proto2 `uint32`/`uint64` field. Distinct from
/// [`write_int32_field`]: unlike `int32`/enum, unsigned fields never
/// sign-extend, so this is a pure (lossless) widening cast.
pub(crate) fn write_uint32_field(out: &mut Vec<u8>, field: u32, value: u32) {
    write_tag(out, field, 0);
    write_varint(out, u64::from(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    enum TestError {
        Truncated,
        InvalidVarint,
        InvalidFieldNumber,
        LengthNotRepresentable,
        UnsupportedWireType(u8),
    }

    impl ProtobufDecodeError for TestError {
        fn truncated() -> Self {
            Self::Truncated
        }
        fn invalid_varint() -> Self {
            Self::InvalidVarint
        }
        fn invalid_field_number() -> Self {
            Self::InvalidFieldNumber
        }
        fn length_not_representable() -> Self {
            Self::LengthNotRepresentable
        }
        fn unsupported_wire_type(wire_type: u8) -> Self {
            Self::UnsupportedWireType(wire_type)
        }
    }

    #[test]
    fn varint_round_trips_single_and_multi_byte_values() {
        for value in [0_u64, 1, 127, 128, 300, u32::MAX.into(), u64::MAX] {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            let mut cursor = 0;
            let decoded: u64 = read_varint::<TestError>(&out, &mut cursor).expect("decode");
            assert_eq!(decoded, value);
            assert_eq!(cursor, out.len());
        }
    }

    #[test]
    fn read_varint_rejects_truncated_and_overlong_input() {
        let mut cursor = 0;
        assert_eq!(
            read_varint::<TestError>(&[0x80], &mut cursor),
            Err(TestError::Truncated)
        );
        let mut cursor = 0;
        assert_eq!(
            read_varint::<TestError>(&[0x80; 10], &mut cursor),
            Err(TestError::InvalidVarint)
        );
    }

    #[test]
    fn zigzag_round_trips_negative_and_positive_values() {
        for value in [0_i64, 1, -1, 2, -2, i32::MIN.into(), i32::MAX.into()] {
            // Zigzag-encode by hand: this is the transform read_zigzag_varint
            // reverses, and this module has no writer for it yet (nothing
            // calls write_zigzag_varint this pass).
            #[allow(clippy::cast_sign_loss)]
            let zigzagged = ((value << 1) ^ (value >> 63)) as u64;
            let mut out = Vec::new();
            write_varint(&mut out, zigzagged);
            let mut cursor = 0;
            let decoded: i64 = read_zigzag_varint::<TestError>(&out, &mut cursor).expect("decode");
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn tag_round_trips_field_and_wire_type() {
        let mut out = Vec::new();
        write_tag(&mut out, 5, 2);
        let mut cursor = 0;
        let (field, wire_type) = read_tag::<TestError>(&out, &mut cursor).expect("decode");
        assert_eq!(field, 5);
        assert_eq!(wire_type, 2);
    }

    #[test]
    fn read_tag_rejects_field_number_zero() {
        let mut out = Vec::new();
        write_tag(&mut out, 0, 0);
        let mut cursor = 0;
        assert_eq!(
            read_tag::<TestError>(&out, &mut cursor),
            Err(TestError::InvalidFieldNumber)
        );
    }

    #[test]
    fn length_delimited_round_trips_and_advances_cursor() {
        let mut out = Vec::new();
        write_tag(&mut out, 3, 2);
        write_varint(&mut out, 5);
        out.extend_from_slice(b"hello");
        let mut cursor = 0;
        let (field, wire_type) = read_tag::<TestError>(&out, &mut cursor).expect("tag");
        assert_eq!((field, wire_type), (3, 2));
        let value = read_length_delimited::<TestError>(&out, &mut cursor).expect("value");
        assert_eq!(value, b"hello");
        assert_eq!(cursor, out.len());
    }

    #[test]
    fn length_delimited_rejects_truncated_value() {
        let mut cursor = 0;
        assert_eq!(
            read_length_delimited::<TestError>(&[0x05, b'h', b'i'], &mut cursor),
            Err(TestError::Truncated)
        );
    }

    #[test]
    fn skip_unknown_field_advances_past_every_supported_wire_type() {
        for (wire_type, bytes) in [
            (0_u8, vec![0x01]),
            (1, vec![0; 8]),
            (2, {
                let mut out = Vec::new();
                write_varint(&mut out, 2);
                out.extend_from_slice(b"hi");
                out
            }),
            (5, vec![0; 4]),
        ] {
            let mut cursor = 0;
            skip_unknown_field::<TestError>(&bytes, &mut cursor, wire_type).expect("skip");
            assert_eq!(cursor, bytes.len());
        }
    }

    #[test]
    fn skip_unknown_field_rejects_unsupported_wire_type() {
        let mut cursor = 0;
        assert_eq!(
            skip_unknown_field::<TestError>(&[], &mut cursor, 3),
            Err(TestError::UnsupportedWireType(3))
        );
    }

    #[test]
    fn write_int32_field_matches_hand_computed_bytes() {
        let mut out = Vec::new();
        write_int32_field(&mut out, 1, 0);
        assert_eq!(out, vec![0x08, 0x00]);
    }

    #[test]
    fn write_uint32_field_matches_hand_computed_bytes_and_round_trips() {
        let mut out = Vec::new();
        write_uint32_field(&mut out, 1, 48_000);
        let mut cursor = 0;
        let (field, wire_type) = read_tag::<TestError>(&out, &mut cursor).expect("tag");
        assert_eq!((field, wire_type), (1, 0));
        let value = read_varint::<TestError>(&out, &mut cursor).expect("value");
        assert_eq!(value, 48_000);
    }

    #[test]
    fn write_int32_field_sign_extends_negative_values_to_ten_bytes() {
        // Matches BluetoothPairingMethod's -1 (docs/protocol/aasdk-adoption.md):
        // a negative int32/enum value is cast through i64, not zigzagged.
        let mut out = Vec::new();
        write_int32_field(&mut out, 2, -1);
        assert_eq!(
            out,
            vec![
                0x10, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01
            ]
        );
    }

    #[test]
    fn write_int32_field_round_trips_through_read_varint() {
        let mut out = Vec::new();
        write_int32_field(&mut out, 7, 300);
        let mut cursor = 0;
        let (field, wire_type) = read_tag::<TestError>(&out, &mut cursor).expect("tag");
        assert_eq!((field, wire_type), (7, 0));
        let value = read_varint::<TestError>(&out, &mut cursor).expect("value");
        assert_eq!(value, 300);
    }

    #[test]
    fn write_length_delimited_field_matches_hand_computed_bytes() {
        let mut out = Vec::new();
        write_length_delimited_field(&mut out, 3, b"hello");
        assert_eq!(out, vec![0x1a, 0x05, b'h', b'e', b'l', b'l', b'o']);
    }
}

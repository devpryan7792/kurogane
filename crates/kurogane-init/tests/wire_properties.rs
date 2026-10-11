//! Property-based testing for Wayland wire protocol parsing, serialization bijectivity,
//! ring buffer stream reassembly, and fuzz resilience.
//! Verified under Domain 1 & Domain 4 Zero-Theater testing criteria.

use kurogane_init::wayland_wire::{
    is_blacklisted_interface, parse_wayland_string, RegistryGlobal, WaylandHeader,
    WireStreamBuffer, BLACKLISTED_INTERFACES,
};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Invariant 1: Header serialization/parsing bijectivity.
    /// For any valid 32-bit object_id, 16-bit opcode, and 16-bit size >= 8,
    /// serialize then parse produces the identical header.
    #[test]
    fn test_header_bijectivity(
        object_id in any::<u32>(),
        opcode in any::<u16>(),
        size in 8u16..=u16::MAX,
    ) {
        let original = WaylandHeader { object_id, opcode, size };
        let mut buf = [0u8; WaylandHeader::SIZE];
        original.serialize(&mut buf);

        let parsed = WaylandHeader::parse(&buf).expect("parse serialized header");
        prop_assert_eq!(original.object_id, parsed.object_id);
        prop_assert_eq!(original.opcode, parsed.opcode);
        prop_assert_eq!(original.size, parsed.size);
    }

    /// Invariant 2: Protocol invariant enforcement: size < 8 MUST be rejected.
    #[test]
    fn test_header_invalid_size_rejection(
        object_id in any::<u32>(),
        opcode in any::<u16>(),
        size in 0u16..8u16,
    ) {
        let invalid = WaylandHeader { object_id, opcode, size };
        let mut buf = [0u8; WaylandHeader::SIZE];
        invalid.serialize(&mut buf);

        prop_assert!(WaylandHeader::parse(&buf).is_none());
    }

    /// Invariant 3: Short buffers (< 8 bytes) MUST always return None without panicking.
    #[test]
    fn test_header_truncated_slice(buf in prop::collection::vec(any::<u8>(), 0..8usize)) {
        prop_assert!(WaylandHeader::parse(&buf).is_none());
    }

    /// Invariant 4: wl_registry::global message serialization and parsing bijectivity.
    /// Wire messages must be 32-bit aligned and deserialization must perfectly restore
    /// name, interface name, and version.
    #[test]
    fn test_registry_global_bijectivity(
        object_id in 1u32..=10000u32,
        name in 1u32..=10000u32,
        interface in "[a-z][a-z0-9_]{1,32}",
        version in 1u32..=10u32,
    ) {
        let mut out = [0u8; 512];
        let bytes_written = RegistryGlobal::serialize_message(
            object_id,
            name,
            &interface,
            version,
            &mut out,
        ).expect("serialize global message");

        // Wayland wire messages MUST be 4-byte aligned
        prop_assert_eq!(bytes_written % 4, 0);
        prop_assert!(bytes_written >= 20);

        // Header verification
        let header = WaylandHeader::parse(&out[..bytes_written]).expect("parse header");
        prop_assert_eq!(header.object_id, object_id);
        prop_assert_eq!(header.opcode, 0);
        prop_assert_eq!(header.size as usize, bytes_written);

        // Payload deserialization
        let payload = &out[WaylandHeader::SIZE..bytes_written];
        let global = RegistryGlobal::parse(payload).expect("parse payload");
        prop_assert_eq!(global.name, name);
        prop_assert_eq!(global.interface, interface.as_str());
        prop_assert_eq!(global.version, version);
    }

    /// Invariant 5: Blacklist filter completeness and sound separation.
    #[test]
    fn test_blacklisted_interfaces_exactness(
        random_suffix in "[a-z0-9_]{1,16}",
    ) {
        for &blacklisted in BLACKLISTED_INTERFACES {
            prop_assert!(is_blacklisted_interface(blacklisted));
            // Suffix alteration must not match
            let modified = format!("{}_{}", blacklisted, random_suffix);
            prop_assert!(!is_blacklisted_interface(&modified));
        }
    }

    /// Invariant 6: WireStreamBuffer arbitrary chunk fragmentation resilience.
    /// Splitting any valid message into 2 arbitrary chunk slices and streaming them
    /// must reconstruct the original message perfectly once complete.
    #[test]
    fn test_wire_stream_buffer_arbitrary_chunking(
        object_id in 1u32..=100u32,
        name in 1u32..=100u32,
        interface in "[a-z][a-z0-9_]{2,20}",
        version in 1u32..=5u32,
        split_ratio in 1usize..99usize,
    ) {
        let mut msg_buf = [0u8; 256];
        let total_len = RegistryGlobal::serialize_message(
            object_id,
            name,
            &interface,
            version,
            &mut msg_buf,
        ).expect("serialize");

        let split_point = 1 + ((total_len - 2) * split_ratio) / 100;
        let mut stream_buf = WireStreamBuffer::<1024>::new();

        // Feed first slice
        let slice1 = &msg_buf[..split_point];
        let space = stream_buf.writable_slice_mut();
        space[..slice1.len()].copy_from_slice(slice1);
        stream_buf.commit_write(slice1.len());

        // Incomplete message MUST yield None
        prop_assert!(stream_buf.next_message().is_none());

        // Feed second slice
        let slice2 = &msg_buf[split_point..total_len];
        let space = stream_buf.writable_slice_mut();
        space[..slice2.len()].copy_from_slice(slice2);
        stream_buf.commit_write(slice2.len());

        // Now must yield the complete reconstructed message
        let reassembled = stream_buf.next_message().expect("reassembled message");
        prop_assert_eq!(reassembled.header.object_id, object_id);
        prop_assert_eq!(reassembled.header.size as usize, total_len);

        let parsed_global = RegistryGlobal::parse(reassembled.payload).expect("parse payload");
        prop_assert_eq!(parsed_global.name, name);
        prop_assert_eq!(parsed_global.interface, interface.as_str());
        prop_assert_eq!(parsed_global.version, version);
        prop_assert_eq!(stream_buf.available(), 0);
    }

    /// Invariant 7: Fuzz robustness against arbitrary noise payloads.
    /// Parser functions MUST NEVER panic or crash when fed random bytes.
    #[test]
    fn test_fuzz_parser_no_panics(
        random_bytes in prop::collection::vec(any::<u8>(), 0..256usize),
        offset in 0usize..64usize,
    ) {
        let _ = WaylandHeader::parse(&random_bytes);
        let _ = RegistryGlobal::parse(&random_bytes);
        let _ = parse_wayland_string(&random_bytes, offset);
    }
}

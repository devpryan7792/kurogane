//! Wayland Binary Wire-Protocol Parser & Demuxer
//! Implements zero-allocation 8-byte framing, streaming chunk reassembly,
//! and registry global capability inspection according to Wayland wire specification.

use crate::{BufferSealPolicy, KuroganeError};

/// The standard 8-byte header present at the start of every Wayland wire message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaylandHeader {
    /// Object ID receiving the request or emitting the event.
    pub object_id: u32,
    /// Opcode identifying the method/event on the interface.
    pub opcode: u16,
    /// Total length of the message in bytes (header + payload). Minimum 8.
    pub size: u16,
}

impl WaylandHeader {
    pub const SIZE: usize = 8;

    /// Attempts to parse an 8-byte header from the beginning of a slice.
    #[inline]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }

        let object_id = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let opcode = u16::from_ne_bytes([buf[4], buf[5]]);
        let size = u16::from_ne_bytes([buf[6], buf[7]]);

        if (size as usize) < Self::SIZE {
            return None;
        }

        Some(Self {
            object_id,
            opcode,
            size,
        })
    }

    /// Serializes header into an 8-byte buffer.
    #[inline]
    pub fn serialize(&self, buf: &mut [u8; Self::SIZE]) {
        buf[0..4].copy_from_slice(&self.object_id.to_ne_bytes());
        buf[4..6].copy_from_slice(&self.opcode.to_ne_bytes());
        buf[6..8].copy_from_slice(&self.size.to_ne_bytes());
    }
}

/// A parsed Wayland wire message referencing the underlying buffer slice (zero-copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandMessage<'a> {
    pub header: WaylandHeader,
    pub payload: &'a [u8],
}

/// A fixed-capacity, zero-allocation container for SCM_RIGHTS file descriptors.
/// In the Wayland wire protocol, clients transmit shared memory fds and dmabufs
/// via Unix domain socket control messages (SCM_RIGHTS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AncillaryFdBuffer<const MAX_FDS: usize = 32> {
    fds: [std::os::unix::io::RawFd; MAX_FDS],
    len: usize,
}

impl<const MAX_FDS: usize> Default for AncillaryFdBuffer<MAX_FDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_FDS: usize> AncillaryFdBuffer<MAX_FDS> {
    pub const fn new() -> Self {
        Self {
            fds: [-1; MAX_FDS],
            len: 0,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[std::os::unix::io::RawFd] {
        &self.fds[..self.len]
    }

    #[inline]
    pub fn push(&mut self, fd: std::os::unix::io::RawFd) -> Result<(), KuroganeError> {
        if self.len >= MAX_FDS {
            return Err(KuroganeError::WaylandProxyError(
                "Ancillary FD buffer overflow: maximum 32 file descriptors",
            ));
        }
        self.fds[self.len] = fd;
        self.len += 1;
        Ok(())
    }

    #[inline]
    pub fn extend_from_slice(&mut self, slice: &[std::os::unix::io::RawFd]) -> Result<(), KuroganeError> {
        if self.len + slice.len() > MAX_FDS {
            return Err(KuroganeError::WaylandProxyError(
                "Ancillary FD buffer overflow on extend",
            ));
        }
        self.fds[self.len..self.len + slice.len()].copy_from_slice(slice);
        self.len += slice.len();
        Ok(())
    }

    #[inline]
    pub fn clear(&mut self) {
        self.fds.fill(-1);
        self.len = 0;
    }
}

/// Blacklisted capability interfaces that the Switchboard Proxy excises from the registry.
pub const BLACKLISTED_INTERFACES: &[&str] = &[
    "wlr_screencopy_v1",
    "wlr_screencopy_manager_v1",
    "zwp_virtual_keyboard_v1",
    "zwp_virtual_keyboard_manager_v1",
    "ext_data_control_v1",
    "ext_data_control_manager_v1",
    "wlr_export_dmabuf_unstable_v1",
    "wlr_export_dmabuf_manager_v1",
];

/// Checks if an advertised interface name is in the security blacklist.
pub fn is_blacklisted_interface(interface: &str) -> bool {
    BLACKLISTED_INTERFACES.contains(&interface)
}

/// Extracts a 32-bit aligned string argument from a Wayland payload.
/// Format: [u32 length including \0][bytes][padding to 4-byte boundary].
pub fn parse_wayland_string(payload: &[u8], offset: usize) -> Option<(&str, usize)> {
    if payload.len() < offset + 4 {
        return None;
    }

    let len = u32::from_ne_bytes([
        payload[offset],
        payload[offset + 1],
        payload[offset + 2],
        payload[offset + 3],
    ]) as usize;

    if len == 0 {
        return Some(("", offset + 4));
    }

    let str_start = offset + 4;
    let str_end = str_start + len;

    if payload.len() < str_end {
        return None;
    }

    // String includes terminating null byte, exclude it for Rust &str
    let raw_bytes = &payload[str_start..str_end - 1];
    let s = std::str::from_utf8(raw_bytes).ok()?;

    // Align consumed length to 32-bit (4-byte) boundary
    let padded_len = (len + 3) & !3;
    let next_offset = str_start + padded_len;

    Some((s, next_offset))
}

/// A parsed wl_registry::global event payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryGlobal<'a> {
    pub name: u32,
    pub interface: &'a str,
    pub version: u32,
}

impl<'a> RegistryGlobal<'a> {
    /// Attempts to parse a wl_registry::global event payload.
    /// Event argument signature: u (name), s (interface), u (version)
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        if payload.len() < 4 {
            return None;
        }

        let name = u32::from_ne_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let (interface, next_offset) = parse_wayland_string(payload, 4)?;

        if payload.len() < next_offset + 4 {
            return None;
        }

        let version = u32::from_ne_bytes([
            payload[next_offset],
            payload[next_offset + 1],
            payload[next_offset + 2],
            payload[next_offset + 3],
        ]);

        Some(Self {
            name,
            interface,
            version,
        })
    }

    /// Serializes a wl_registry::global message into a buffer, returning total bytes written.
    pub fn serialize_message(
        object_id: u32,
        name: u32,
        interface: &str,
        version: u32,
        out: &mut [u8],
    ) -> Result<usize, KuroganeError> {
        let str_bytes = interface.as_bytes();
        let str_len = str_bytes.len() + 1; // including \0
        let padded_len = (str_len + 3) & !3;
        let payload_size = 4 + 4 + padded_len + 4; // name (4) + strlen (4) + str + version (4)
        let total_size = WaylandHeader::SIZE + payload_size;

        if out.len() < total_size {
            return Err(KuroganeError::WaylandProxyError(
                "Buffer too small for global message",
            ));
        }

        // Header: opcode 0 for wl_registry::global
        let header = WaylandHeader {
            object_id,
            opcode: 0,
            size: total_size as u16,
        };
        let mut hdr_buf = [0u8; 8];
        header.serialize(&mut hdr_buf);
        out[0..8].copy_from_slice(&hdr_buf);

        // Name
        out[8..12].copy_from_slice(&name.to_ne_bytes());

        // String length
        out[12..16].copy_from_slice(&(str_len as u32).to_ne_bytes());

        // String bytes
        out[16..16 + str_bytes.len()].copy_from_slice(str_bytes);
        out[16 + str_bytes.len()] = 0; // null terminator

        // Zero out padding
        out[16 + str_len..16 + padded_len].fill(0);

        // Version
        let ver_offset = 16 + padded_len;
        out[ver_offset..ver_offset + 4].copy_from_slice(&version.to_ne_bytes());

        Ok(total_size)
    }
}

/// Ring-buffer streaming reassembler to parse fragmented Wayland messages from socket streams.
pub struct WireStreamBuffer<const CAPACITY: usize = 8192> {
    buf: [u8; CAPACITY],
    read_pos: usize,
    write_pos: usize,
}

impl<const CAPACITY: usize> Default for WireStreamBuffer<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> WireStreamBuffer<CAPACITY> {
    pub fn new() -> Self {
        Self {
            buf: [0u8; CAPACITY],
            read_pos: 0,
            write_pos: 0,
        }
    }

    /// Amount of buffered, unconsumed bytes currently stored.
    pub fn available(&self) -> usize {
        self.write_pos - self.read_pos
    }

    /// Available contiguous space in buffer to append newly received bytes from socket.
    pub fn writable_slice_mut(&mut self) -> &mut [u8] {
        // If buffer has drained to end, compact to start
        if self.read_pos > 0 && self.read_pos == self.write_pos {
            self.read_pos = 0;
            self.write_pos = 0;
        } else if self.write_pos == CAPACITY {
            // Shift unconsumed slice to index 0
            self.buf.copy_within(self.read_pos..self.write_pos, 0);
            self.write_pos -= self.read_pos;
            self.read_pos = 0;
        }
        &mut self.buf[self.write_pos..CAPACITY]
    }

    /// Advance write pointer by bytes written from socket `recv`.
    pub fn commit_write(&mut self, bytes_written: usize) {
        self.write_pos = std::cmp::min(self.write_pos + bytes_written, CAPACITY);
    }

    /// Inspects unconsumed buffer; returns next complete message if fully received.
    pub fn next_message(&mut self) -> Option<WaylandMessage<'_>> {
        let unconsumed = &self.buf[self.read_pos..self.write_pos];
        let header = WaylandHeader::parse(unconsumed)?;
        let msg_size = header.size as usize;

        if unconsumed.len() < msg_size {
            return None; // Message is fragmented; awaiting more bytes from socket
        }

        let payload_start = self.read_pos + WaylandHeader::SIZE;
        let payload_end = self.read_pos + msg_size;

        // Advance read position past this message
        self.read_pos += msg_size;

        Some(WaylandMessage {
            header,
            payload: &self.buf[payload_start..payload_end],
        })
    }

    /// Drains all available buffered bytes as a raw slice.
    pub fn next_raw_slice(&mut self) -> &[u8] {
        let start = self.read_pos;
        let end = self.write_pos;
        self.read_pos = self.write_pos;
        &self.buf[start..end]
    }
}

/// Represents the classification of an incoming SCM_RIGHTS file descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdType {
    /// Linux memfd or shm backing buffer
    SharedMemory,
    /// Direct Rendering Manager DMA buffer (prime)
    DmaBuf,
    /// Unix pipe or standard socket
    PipeOrSocket,
    /// Unknown or regular file descriptor
    Other,
}

/// Inspection outcome for an individual file descriptor passing through SCM_RIGHTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdSecurityReport {
    pub fd: std::os::unix::io::RawFd,
    pub fd_type: FdType,
    pub seals: i32,
    pub is_sealed: bool,
}

/// Inspects an incoming file descriptor and enforces required buffer seals according to policy.
pub fn inspect_and_enforce_fd(
    fd: std::os::unix::io::RawFd,
    policy: BufferSealPolicy,
) -> Result<FdSecurityReport, KuroganeError> {
    // Query seals via fcntl(fd, F_GET_SEALS)
    let seals = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
    if seals >= 0 {
        let required = libc::F_SEAL_SHRINK | libc::F_SEAL_GROW;
        let has_required_seals = (seals & required) == required;

        if !has_required_seals {
            match policy {
                BufferSealPolicy::RejectUnsealed => {
                    return Err(KuroganeError::MemfdSealError(
                        "Rejected unsealed memfd: missing F_SEAL_SHRINK | F_SEAL_GROW",
                    ));
                }
                BufferSealPolicy::AutoApplySeals => {
                    let ret = unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, required) };
                    if ret < 0 {
                        return Err(KuroganeError::MemfdSealError(
                            "Failed to apply required memory seals to memfd",
                        ));
                    }
                    return Ok(FdSecurityReport {
                        fd,
                        fd_type: FdType::SharedMemory,
                        seals: seals | required,
                        is_sealed: true,
                    });
                }
                BufferSealPolicy::Disabled => {}
            }
        }

        Ok(FdSecurityReport {
            fd,
            fd_type: FdType::SharedMemory,
            seals,
            is_sealed: has_required_seals,
        })
    } else {
        // Not a sealed memfd (could be a dmabuf, eventfd, or pipe)
        Ok(FdSecurityReport {
            fd,
            fd_type: FdType::Other,
            seals: 0,
            is_sealed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization_and_parsing() {
        let original = WaylandHeader {
            object_id: 42,
            opcode: 3,
            size: 24,
        };

        let mut buf = [0u8; 8];
        original.serialize(&mut buf);

        let parsed = WaylandHeader::parse(&buf).expect("parse header");
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_invalid_header_rejection() {
        // Buffer < 8 bytes
        assert_eq!(WaylandHeader::parse(&[1, 2, 3]), None);

        // Size < 8 bytes (violates Wayland wire invariants)
        let invalid_size = WaylandHeader {
            object_id: 1,
            opcode: 0,
            size: 4,
        };
        let mut buf = [0u8; 8];
        invalid_size.serialize(&mut buf);
        assert_eq!(WaylandHeader::parse(&buf), None);
    }

    #[test]
    fn test_registry_global_serialization_and_parsing() {
        let mut out = [0u8; 64];
        let bytes_written = RegistryGlobal::serialize_message(
            2,               // registry object ID
            1,               // global name
            "wl_compositor", // interface
            4,               // version
            &mut out,
        )
        .expect("serialize global");

        let header = WaylandHeader::parse(&out[..bytes_written]).expect("parse header");
        assert_eq!(header.object_id, 2);
        assert_eq!(header.opcode, 0); // wl_registry::global is opcode 0
        assert_eq!(header.size as usize, bytes_written);

        let global = RegistryGlobal::parse(&out[8..bytes_written]).expect("parse global payload");
        assert_eq!(global.name, 1);
        assert_eq!(global.interface, "wl_compositor");
        assert_eq!(global.version, 4);
    }

    #[test]
    fn test_blacklisted_interface_detection() {
        assert!(is_blacklisted_interface("wlr_screencopy_v1"));
        assert!(is_blacklisted_interface("zwp_virtual_keyboard_v1"));
        assert!(is_blacklisted_interface("ext_data_control_v1"));
        assert!(!is_blacklisted_interface("wl_compositor"));
        assert!(!is_blacklisted_interface("wl_shm"));
        assert!(!is_blacklisted_interface("wl_seat"));
    }

    #[test]
    fn test_wire_stream_buffer_fragmentation_handling() {
        let mut stream_buf = WireStreamBuffer::<1024>::new();

        // Prepare a message
        let mut full_msg = [0u8; 40];
        let len = RegistryGlobal::serialize_message(2, 5, "wl_shm", 1, &mut full_msg).unwrap();

        // Feed chunk 1: only first 10 bytes (header + partial payload)
        let space = stream_buf.writable_slice_mut();
        space[..10].copy_from_slice(&full_msg[..10]);
        stream_buf.commit_write(10);

        // Next message should return None because message is incomplete
        assert!(stream_buf.next_message().is_none());

        // Feed chunk 2: remaining bytes
        let space = stream_buf.writable_slice_mut();
        space[..len - 10].copy_from_slice(&full_msg[10..len]);
        stream_buf.commit_write(len - 10);

        // Now next_message should successfully yield the complete message!
        let msg = stream_buf
            .next_message()
            .expect("reassembled complete message");
        assert_eq!(msg.header.object_id, 2);
        assert_eq!(msg.header.size as usize, len);

        let global = RegistryGlobal::parse(msg.payload).expect("parse payload");
        assert_eq!(global.interface, "wl_shm");
        assert_eq!(global.name, 5);

        // Stream buffer should now be empty
        assert_eq!(stream_buf.available(), 0);
    }

    #[test]
    fn test_ancillary_fd_buffer_push_and_overflow() {
        let mut buf = AncillaryFdBuffer::<4>::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);

        buf.push(10).unwrap();
        buf.push(11).unwrap();
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.as_slice(), &[10, 11]);

        buf.extend_from_slice(&[12, 13]).unwrap();
        assert_eq!(buf.len(), 4);
        assert_eq!(buf.as_slice(), &[10, 11, 12, 13]);

        // Push past capacity must fail gracefully
        assert!(buf.push(14).is_err());

        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_inspect_and_enforce_fd_with_memfd() {
        use std::ffi::CString;

        let name = CString::new("test_shm").unwrap();
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_ALLOW_SEALING) };
        if fd >= 0 {
            // Initially, memfd has no seals
            let report_unsealed = inspect_and_enforce_fd(fd, BufferSealPolicy::Disabled).unwrap();
            assert_eq!(report_unsealed.fd_type, FdType::SharedMemory);
            assert!(!report_unsealed.is_sealed);

            // Reject policy must reject
            let reject_err = inspect_and_enforce_fd(fd, BufferSealPolicy::RejectUnsealed);
            assert!(reject_err.is_err());

            // Auto-apply policy must apply seals and succeed
            let report_sealed = inspect_and_enforce_fd(fd, BufferSealPolicy::AutoApplySeals).unwrap();
            assert_eq!(report_sealed.fd_type, FdType::SharedMemory);
            assert!(report_sealed.is_sealed);

            // Now reject policy must succeed because seals are active!
            let report_now_ok = inspect_and_enforce_fd(fd, BufferSealPolicy::RejectUnsealed).unwrap();
            assert!(report_now_ok.is_sealed);

            unsafe { libc::close(fd) };
        }
    }
}

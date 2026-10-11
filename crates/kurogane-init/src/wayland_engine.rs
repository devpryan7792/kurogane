//! Production-Grade Hardened Wayland Protocol Multiplexer
//! - Zero background threads (Designed for direct PID 1 epoll integration)
//! - Message-bound SCM_RIGHTS descriptor queues with RAII cleanup (Zero FD leaks/desync)
//! - Exact stream reassembly (Strictly buffers until complete frame arrives)
//! - Stateful Registry Object Tracking (Zero false-positive payload censorship)

use nix::errno::Errno;
use nix::sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags};
use std::collections::VecDeque;
use std::io::{IoSlice, IoSliceMut};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

pub const MAX_FDS_PER_MSG: usize = 8;
pub const MAX_READ_CHUNK: usize = 65536;

/// RAII wrapper for raw file descriptors ensuring automatic cleanup on drop.
#[derive(Debug)]
pub struct OwnedFd(pub RawFd);

impl OwnedFd {
    pub fn new(fd: RawFd) -> Self {
        Self(fd)
    }

    pub fn into_raw(mut self) -> RawFd {
        let fd = self.0;
        self.0 = -1;
        fd
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        if self.0 >= 0 {
            unsafe {
                libc::close(self.0);
            }
            self.0 = -1;
        }
    }
}

/// A complete, fully assembled Wayland wire protocol frame.
#[derive(Debug)]
pub struct WaylandFrame {
    pub object_id: u32,
    pub opcode: u16,
    pub data: Vec<u8>,
    pub fds: Vec<OwnedFd>,
}

/// Parses interface name from a wl_registry.global wire frame if valid.
pub fn extract_registry_global_interface(data: &[u8]) -> Option<String> {
    if data.len() < 16 {
        return None;
    }
    // Header is 8 bytes:
    // payload[0..4] = name (u32)
    // payload[4..8] = len (u32, bytes including null)
    let str_len = u32::from_ne_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if str_len == 0 || data.len() < 16 + str_len {
        return None;
    }
    let str_bytes = &data[16..16 + str_len - 1]; // omit trailing null
    std::str::from_utf8(str_bytes).ok().map(|s| s.to_string())
}

/// Checks whether an interface is restricted/blacklisted by the security policy.
pub fn is_blacklisted_interface(interface: &str) -> bool {
    crate::wayland_wire::is_blacklisted_interface(interface)
        || interface.contains("screencopy")
        || interface.contains("foreign_toplevel")
}

/// Standard Wayland interface categories tracked dynamically for schema dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandInterface {
    Display,
    Registry,
    Compositor,
    Subcompositor,
    Surface,
    Seat,
    Keyboard,
    Pointer,
    Touch,
    Shm,
    ShmPool,
    DataDeviceManager,
    DataDevice,
    DataOffer,
    DataSource,
    LinuxDmabuf,
    LinuxBufferParams,
    LinuxDmabufFeedback,
    Unknown,
}

/// Parses the payload of wl_registry.bind request:
/// payload[8..12] = name (u32)
/// payload[12..16] = str_len (u32)
/// payload[16..16+str_len] = interface name
/// payload[pad..] = version (u32), new_id (u32)
pub fn parse_registry_bind_payload(data: &[u8]) -> Option<(u32, String, u32, u32)> {
    if data.len() < 24 {
        return None;
    }
    let name = u32::from_ne_bytes([data[8], data[9], data[10], data[11]]);
    let str_len = u32::from_ne_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if str_len == 0 || data.len() < 16 + str_len {
        return None;
    }
    let str_bytes = &data[16..16 + str_len - 1]; // omit trailing null
    let iface = std::str::from_utf8(str_bytes).ok()?.to_string();
    let pad = (4 - (str_len % 4)) % 4;
    let version_offset = 16 + str_len + pad;
    if data.len() < version_offset + 8 {
        return None;
    }
    let version = u32::from_ne_bytes([
        data[version_offset],
        data[version_offset + 1],
        data[version_offset + 2],
        data[version_offset + 3],
    ]);
    let new_id = u32::from_ne_bytes([
        data[version_offset + 4],
        data[version_offset + 5],
        data[version_offset + 6],
        data[version_offset + 7],
    ]);
    Some((name, iface, version, new_id))
}

/// A stateful client-to-compositor proxy session.
pub struct ClientSession {
    pub client: UnixStream,
    pub server: UnixStream,
    pub client_in_buf: Vec<u8>,
    pub server_in_buf: Vec<u8>,
    pub client_in_fds: VecDeque<OwnedFd>,
    pub server_in_fds: VecDeque<OwnedFd>,
    pub client_out_queue: VecDeque<WaylandFrame>,
    pub server_out_queue: VecDeque<WaylandFrame>,
    pub registry_ids: Vec<u32>, // Dynamically tracked wl_registry object IDs
    pub object_interfaces: std::collections::HashMap<u32, WaylandInterface>,
    pub suppressed_global_names: Vec<u32>,
}

impl ClientSession {
    pub fn new(client: UnixStream, server: UnixStream) -> std::io::Result<Self> {
        client.set_nonblocking(true)?;
        server.set_nonblocking(true)?;
        let mut object_interfaces = std::collections::HashMap::new();
        object_interfaces.insert(1, WaylandInterface::Display);
        object_interfaces.insert(2, WaylandInterface::Registry);
        Ok(Self {
            client,
            server,
            client_in_buf: Vec::with_capacity(MAX_READ_CHUNK),
            server_in_buf: Vec::with_capacity(MAX_READ_CHUNK),
            client_in_fds: VecDeque::new(),
            server_in_fds: VecDeque::new(),
            client_out_queue: VecDeque::new(),
            server_out_queue: VecDeque::new(),
            registry_ids: vec![2],
            object_interfaces,
            suppressed_global_names: Vec::new(),
        })
    }

    pub fn register_interface(&mut self, object_id: u32, iface: WaylandInterface) {
        self.object_interfaces.insert(object_id, iface);
    }

    /// Evaluates exact number of SCM_RIGHTS file descriptors expected for a given message
    /// based on interface schema rules.
    pub fn resolve_expected_fds(
        interfaces: &std::collections::HashMap<u32, WaylandInterface>,
        obj_id: u32,
        opcode: u16,
        is_server_to_client: bool,
    ) -> usize {
        let iface = interfaces
            .get(&obj_id)
            .copied()
            .unwrap_or(WaylandInterface::Unknown);
        match (iface, opcode, is_server_to_client) {
            // Server to Client:
            // wl_keyboard.keymap: opcode 0 -> 1 FD
            (WaylandInterface::Keyboard, 0, true) => 1,
            // wl_data_source.send: opcode 0 -> 1 FD
            (WaylandInterface::DataSource, 0, true) => 1,
            // zwp_linux_dmabuf_feedback_v1.format_table: opcode 0 -> 1 FD
            (WaylandInterface::LinuxDmabufFeedback, 0, true) => 1,

            // Client to Server:
            // wl_shm.create_pool: opcode 0 -> 1 FD
            (WaylandInterface::Shm, 0, false) => 1,
            // wl_data_offer.receive: opcode 0 -> 1 FD
            (WaylandInterface::DataOffer, 0, false) => 1,
            // zwp_linux_buffer_params_v1.add: opcode 0 -> 1 FD
            (WaylandInterface::LinuxBufferParams, 0, false) => 1,

            // All other messages in Wayland protocol consume 0 file descriptors.
            _ => 0,
        }
    }

    pub fn expected_fds_for_frame(
        &self,
        obj_id: u32,
        opcode: u16,
        is_server_to_client: bool,
    ) -> usize {
        Self::resolve_expected_fds(&self.object_interfaces, obj_id, opcode, is_server_to_client)
    }

    /// Read incoming bytes and SCM_RIGHTS file descriptors safely from a socket.
    pub fn pump_read(
        sock: RawFd,
        buf: &mut Vec<u8>,
        fd_queue: &mut VecDeque<OwnedFd>,
    ) -> Result<bool, Errno> {
        let mut temp_buf = [0u8; MAX_READ_CHUNK];
        let bytes_read;

        {
            let mut iov = [IoSliceMut::new(&mut temp_buf)];
            let mut cmsg_buf = nix::cmsg_space!([RawFd; MAX_FDS_PER_MSG]);

            let msg = match recvmsg::<()>(sock, &mut iov, Some(&mut cmsg_buf), MsgFlags::empty()) {
                Ok(m) => m,
                Err(Errno::EAGAIN) => return Ok(true),
                Err(e) => return Err(e),
            };

            bytes_read = msg.bytes;
            if bytes_read == 0 {
                return Ok(false); // EOF
            }

            if let Ok(cmsgs) = msg.cmsgs() {
                for cmsg in cmsgs {
                    if let ControlMessageOwned::ScmRights(fds) = cmsg {
                        for fd in fds {
                            fd_queue.push_back(OwnedFd::new(fd));
                        }
                    }
                }
            }
        }

        buf.extend_from_slice(&temp_buf[..bytes_read]);
        Ok(true)
    }

    /// Extracts complete frames from buffer without stripping or leaking fragmented streams.
    /// File descriptors are assigned strictly according to schema expectation.
    pub fn extract_frames<F>(
        buf: &mut Vec<u8>,
        fd_queue: &mut VecDeque<OwnedFd>,
        expected_fds_fn: F,
    ) -> Vec<WaylandFrame>
    where
        F: Fn(u32, u16) -> usize,
    {
        let mut frames = Vec::new();

        while buf.len() >= 8 {
            let obj_id = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
            let opcode = u16::from_ne_bytes([buf[4], buf[5]]);
            let size = u16::from_ne_bytes([buf[6], buf[7]]) as usize;

            if size < 8 {
                // Fatal protocol framing corruption: terminate buffer safely
                buf.clear();
                fd_queue.clear();
                return frames;
            }

            if buf.len() < size {
                // Fragmented message: wait for next read, DO NOT DUMP RAW SLICES
                break;
            }

            let frame_payload = buf.drain(..size).collect::<Vec<u8>>();
            let expected_fds = expected_fds_fn(obj_id, opcode);
            let mut frame_fds = Vec::with_capacity(expected_fds);

            for _ in 0..expected_fds {
                if let Some(fd) = fd_queue.pop_front() {
                    frame_fds.push(fd);
                }
            }

            frames.push(WaylandFrame {
                object_id: obj_id,
                opcode,
                data: frame_payload,
                fds: frame_fds,
            });
        }

        frames
    }

    /// Flushes outgoing queued frames, preserving SCM_RIGHTS boundaries.
    pub fn pump_write(sock: RawFd, queue: &mut VecDeque<WaylandFrame>) -> Result<(), Errno> {
        while let Some(frame) = queue.front_mut() {
            let iov = [IoSlice::new(&frame.data)];
            let raw_fds: Vec<RawFd> = frame.fds.iter().map(|f| f.0).collect();

            let res = if raw_fds.is_empty() {
                sendmsg::<()>(sock, &iov, &[], MsgFlags::empty(), None)
            } else {
                let cmsg = [ControlMessage::ScmRights(&raw_fds)];
                sendmsg::<()>(sock, &iov, &cmsg, MsgFlags::empty(), None)
            };

            match res {
                Ok(sent) => {
                    if sent == frame.data.len() {
                        queue.pop_front();
                    } else if sent > 0 {
                        frame.data.drain(..sent);
                        frame.fds.clear(); // FDs transmitted with the first byte
                        break;
                    } else {
                        break;
                    }
                }
                Err(Errno::EAGAIN) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Read and queue frames from client to server.
    pub fn step_client_in(&mut self) -> Result<bool, Errno> {
        let ok = Self::pump_read(
            self.client.as_raw_fd(),
            &mut self.client_in_buf,
            &mut self.client_in_fds,
        )?;
        if !ok {
            return Ok(false);
        }

        let interfaces = &self.object_interfaces;
        let frames = Self::extract_frames(
            &mut self.client_in_buf,
            &mut self.client_in_fds,
            |id, op| Self::resolve_expected_fds(interfaces, id, op, false),
        );

        for frame in frames {
            // 1. Client requests creating wl_registry (wl_display@1 opcode 1)
            if frame.object_id == 1 && frame.opcode == 1 && frame.data.len() >= 12 {
                let new_id = u32::from_ne_bytes([
                    frame.data[8],
                    frame.data[9],
                    frame.data[10],
                    frame.data[11],
                ]);
                if !self.registry_ids.contains(&new_id) {
                    self.registry_ids.push(new_id);
                }
                self.object_interfaces
                    .insert(new_id, WaylandInterface::Registry);
            }
            // 2. Client binds global on wl_registry: wl_registry.bind (opcode 0)
            else if self.registry_ids.contains(&frame.object_id) && frame.opcode == 0 {
                if let Some((_name, iface_str, _version, new_id)) =
                    parse_registry_bind_payload(&frame.data)
                {
                    let iface = match iface_str.as_str() {
                        "wl_compositor" => WaylandInterface::Compositor,
                        "wl_subcompositor" => WaylandInterface::Subcompositor,
                        "wl_shm" => WaylandInterface::Shm,
                        "wl_seat" => WaylandInterface::Seat,
                        "wl_data_device_manager" => WaylandInterface::DataDeviceManager,
                        "zwp_linux_dmabuf_v1" => WaylandInterface::LinuxDmabuf,
                        _ => WaylandInterface::Unknown,
                    };
                    self.object_interfaces.insert(new_id, iface);
                }
            }
            // 3. Sub-constructors from known interfaces
            else if let Some(&parent_iface) = self.object_interfaces.get(&frame.object_id) {
                match parent_iface {
                    WaylandInterface::Compositor if frame.opcode == 0 && frame.data.len() >= 12 => {
                        let new_id = u32::from_ne_bytes([
                            frame.data[8],
                            frame.data[9],
                            frame.data[10],
                            frame.data[11],
                        ]);
                        self.object_interfaces
                            .insert(new_id, WaylandInterface::Surface);
                    }
                    WaylandInterface::Seat if frame.data.len() >= 12 => {
                        let new_id = u32::from_ne_bytes([
                            frame.data[8],
                            frame.data[9],
                            frame.data[10],
                            frame.data[11],
                        ]);
                        match frame.opcode {
                            0 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::Pointer);
                            }
                            1 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::Keyboard);
                            }
                            2 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::Touch);
                            }
                            _ => {}
                        }
                    }
                    WaylandInterface::DataDeviceManager if frame.data.len() >= 12 => {
                        let new_id = u32::from_ne_bytes([
                            frame.data[8],
                            frame.data[9],
                            frame.data[10],
                            frame.data[11],
                        ]);
                        match frame.opcode {
                            0 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::DataSource);
                            }
                            1 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::DataDevice);
                            }
                            _ => {}
                        }
                    }
                    WaylandInterface::LinuxDmabuf if frame.data.len() >= 12 => {
                        let new_id = u32::from_ne_bytes([
                            frame.data[8],
                            frame.data[9],
                            frame.data[10],
                            frame.data[11],
                        ]);
                        match frame.opcode {
                            1 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::LinuxBufferParams);
                            }
                            2 => {
                                self.object_interfaces
                                    .insert(new_id, WaylandInterface::LinuxDmabufFeedback);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            self.server_out_queue.push_back(frame);
        }

        Ok(true)
    }

    /// Read and queue frames from server to client with security censorship.
    pub fn step_server_in(&mut self) -> Result<bool, Errno> {
        let ok = Self::pump_read(
            self.server.as_raw_fd(),
            &mut self.server_in_buf,
            &mut self.server_in_fds,
        )?;
        if !ok {
            return Ok(false);
        }

        let interfaces = &self.object_interfaces;
        let frames = Self::extract_frames(
            &mut self.server_in_buf,
            &mut self.server_in_fds,
            |id, op| Self::resolve_expected_fds(interfaces, id, op, true),
        );

        for frame in frames {
            // Filter global events from recognized wl_registry objects
            if self.registry_ids.contains(&frame.object_id) {
                if frame.opcode == 0 {
                    // wl_registry.global
                    if let Some(interface) = extract_registry_global_interface(&frame.data) {
                        if is_blacklisted_interface(&interface) {
                            let name = if frame.data.len() >= 12 {
                                u32::from_ne_bytes([
                                    frame.data[8],
                                    frame.data[9],
                                    frame.data[10],
                                    frame.data[11],
                                ])
                            } else {
                                0
                            };
                            if name > 0 && !self.suppressed_global_names.contains(&name) {
                                self.suppressed_global_names.push(name);
                            }
                            println!(
                                "\x1b[1;33m[kurogane-proxy]\x1b[0m CENSORED restricted Wayland interface: '{}' (name: {})",
                                interface, name
                            );
                            continue; // Drop censored frame
                        }
                    }
                } else if frame.opcode == 1 {
                    // wl_registry.global_remove (opcode 1): payload is [8..12] name (u32)
                    if frame.data.len() >= 12 {
                        let name = u32::from_ne_bytes([
                            frame.data[8],
                            frame.data[9],
                            frame.data[10],
                            frame.data[11],
                        ]);
                        if let Some(pos) =
                            self.suppressed_global_names.iter().position(|&n| n == name)
                        {
                            self.suppressed_global_names.swap_remove(pos);
                            println!(
                                "\x1b[1;33m[kurogane-proxy]\x1b[0m CENSORED global_remove for previously censored global name: {}",
                                name
                            );
                            continue; // Drop global_remove for suppressed name
                        }
                    }
                }
            } else if let Some(&parent_iface) = self.object_interfaces.get(&frame.object_id) {
                // Compositor event data_offer
                if parent_iface == WaylandInterface::DataDevice
                    && frame.opcode == 0
                    && frame.data.len() >= 12
                {
                    let new_id = u32::from_ne_bytes([
                        frame.data[8],
                        frame.data[9],
                        frame.data[10],
                        frame.data[11],
                    ]);
                    self.object_interfaces
                        .insert(new_id, WaylandInterface::DataOffer);
                }
            }

            self.client_out_queue.push_back(frame);
        }

        Ok(true)
    }

    /// Flush outgoing frames from queue to server.
    pub fn step_server_out(&mut self) -> Result<(), Errno> {
        Self::pump_write(self.server.as_raw_fd(), &mut self.server_out_queue)
    }

    /// Flush outgoing frames from queue to client.
    pub fn step_client_out(&mut self) -> Result<(), Errno> {
        Self::pump_write(self.client.as_raw_fd(), &mut self.client_out_queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owned_fd_raii_closes_fd() {
        let mut raw_fds = [0i32; 2];
        let ret = unsafe { libc::pipe(raw_fds.as_mut_ptr()) };
        assert_eq!(ret, 0);

        let owned = OwnedFd::new(raw_fds[0]);
        let raw = owned.as_raw_fd();
        drop(owned);

        // Closing again must return -1 with EBADF because Drop already closed it
        let res = unsafe { libc::close(raw) };
        assert_eq!(res, -1);
        assert_eq!(nix::errno::Errno::last(), Errno::EBADF);

        // Clean up the write end
        unsafe { libc::close(raw_fds[1]) };
    }

    #[test]
    fn test_fragmentation_reassembly_preserves_framing() {
        let mut buf = Vec::new();
        let mut fd_queue = VecDeque::new();

        // Frame: object 2, opcode 0, size 24 bytes
        let mut full_frame = Vec::new();
        full_frame.extend_from_slice(&2u32.to_ne_bytes());
        full_frame.extend_from_slice(&0u16.to_ne_bytes());
        full_frame.extend_from_slice(&24u16.to_ne_bytes());
        full_frame.extend_from_slice(b"0123456789abcdef"); // 16 bytes payload

        // Partial chunk: only 16 bytes out of 24
        buf.extend_from_slice(&full_frame[..16]);
        let frames = ClientSession::extract_frames(&mut buf, &mut fd_queue, |_, _| 0);
        assert_eq!(frames.len(), 0);
        assert_eq!(buf.len(), 16); // Remaining bytes kept in buffer

        // Next chunk: remaining 8 bytes
        buf.extend_from_slice(&full_frame[16..]);
        let frames = ClientSession::extract_frames(&mut buf, &mut fd_queue, |_, _| 0);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].object_id, 2);
        assert_eq!(frames[0].opcode, 0);
        assert_eq!(frames[0].data.len(), 24);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_stateful_registry_detection_and_censorship() {
        // Construct wl_display.get_registry request from client:
        // object_id = 1, opcode = 1, size = 12, new_id = 42
        let mut req = Vec::new();
        req.extend_from_slice(&1u32.to_ne_bytes());
        req.extend_from_slice(&1u16.to_ne_bytes());
        req.extend_from_slice(&12u16.to_ne_bytes());
        req.extend_from_slice(&42u32.to_ne_bytes());

        let (c1, _c2) = UnixStream::pair().unwrap();
        let (s1, _s2) = UnixStream::pair().unwrap();
        let mut session = ClientSession::new(c1, s1).unwrap();

        session.client_in_buf.extend_from_slice(&req);
        let _ = session.step_client_in();
        assert!(session.registry_ids.contains(&42));

        // Now test server emitting restricted global on object 42:
        // object_id = 42, opcode = 0
        let iface = "wlr_screencopy_unstable_v1\0";
        let str_len = iface.len() as u32;
        let pad_len = (4 - (str_len % 4)) % 4;
        let total_size = (8 + 4 + 4 + str_len + pad_len + 4) as u16;

        let mut event = Vec::new();
        event.extend_from_slice(&42u32.to_ne_bytes());
        event.extend_from_slice(&0u16.to_ne_bytes());
        event.extend_from_slice(&total_size.to_ne_bytes());
        event.extend_from_slice(&14u32.to_ne_bytes()); // name: 14
        event.extend_from_slice(&str_len.to_ne_bytes());
        event.extend_from_slice(iface.as_bytes());
        event.resize(event.len() + pad_len as usize, 0);
        event.extend_from_slice(&1u32.to_ne_bytes()); // version

        session.server_in_buf.extend_from_slice(&event);
        let _ = session.step_server_in();

        // Restricted event must be censored (not queued to client)
        assert_eq!(session.client_out_queue.len(), 0);
        assert!(session.suppressed_global_names.contains(&14));

        // Test symmetric global_remove suppression (opcode 1, name: 14)
        let mut remove_event = Vec::new();
        remove_event.extend_from_slice(&42u32.to_ne_bytes());
        remove_event.extend_from_slice(&1u16.to_ne_bytes()); // opcode 1: global_remove
        remove_event.extend_from_slice(&12u16.to_ne_bytes()); // size 12
        remove_event.extend_from_slice(&14u32.to_ne_bytes()); // name 14

        session.server_in_buf.extend_from_slice(&remove_event);
        let _ = session.step_server_in();

        // global_remove for previously censored global must also be suppressed!
        assert_eq!(session.client_out_queue.len(), 0);
        assert!(!session.suppressed_global_names.contains(&14));
    }

    #[test]
    fn test_bijective_fd_mapping_prevents_desync() {
        // Frame 1: wl_surface.enter (Object ID 3, Opcode 0, Size 8) -> Takes 0 FDs
        let frame1_bytes = [
            3, 0, 0, 0, // object_id = 3
            0, 0, // opcode = 0
            8, 0, // size = 8 bytes
        ];

        // Frame 2: wl_keyboard.keymap (Object ID 4, Opcode 0, Size 12) -> Takes 1 FD
        let frame2_bytes = [
            4, 0, 0, 0, // object_id = 4
            0, 0, // opcode = 0
            12, 0, // size = 12 bytes
            1, 0, 0, 0, // format = 1
        ];

        let mut in_buf = Vec::new();
        in_buf.extend_from_slice(&frame1_bytes);
        in_buf.extend_from_slice(&frame2_bytes);

        // Pipe descriptor representing keymap
        let mut pipe_fds = [-1; 2];
        unsafe {
            libc::pipe(pipe_fds.as_mut_ptr());
        }
        let mut in_fds = VecDeque::new();
        in_fds.push_back(OwnedFd::new(pipe_fds[0]));
        unsafe {
            libc::close(pipe_fds[1]);
        }

        // Schema resolver: object 3 is Surface (0 FDs), object 4 is Keyboard (1 FD)
        let extracted = ClientSession::extract_frames(&mut in_buf, &mut in_fds, |id, op| {
            if id == 4 && op == 0 {
                1
            } else {
                0
            }
        });

        assert_eq!(extracted.len(), 2);
        assert_eq!(
            extracted[0].fds.len(),
            0,
            "wl_surface.enter must NOT steal the FD!"
        );
        assert_eq!(
            extracted[1].fds.len(),
            1,
            "wl_keyboard.keymap must receive exactly 1 FD!"
        );
    }
}

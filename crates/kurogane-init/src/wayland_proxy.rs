use crate::wayland_wire::{
    is_blacklisted_interface, AncillaryFdBuffer, RegistryGlobal, WireStreamBuffer,
};
use crate::{BufferSealPolicy, KuroganeError};
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags, EpollTimeout};
use nix::sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags};
use std::io::{IoSlice, IoSliceMut};
use std::os::unix::io::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;

/// Result of attempting to relay a packet across non-blocking sockets.
#[derive(Debug, PartialEq, Eq)]
pub enum RelayStatus {
    Forwarded(usize),
    WouldBlock,
    Eof,
}

/// Forwards data between non-blocking sockets with SCM_RIGHTS file descriptor preservation (defaults to AutoApplySeals).
pub fn relay_packet(
    src: RawFd,
    dst: RawFd,
    buf: &mut [u8],
    ancillary: &mut AncillaryFdBuffer,
) -> Result<RelayStatus, KuroganeError> {
    relay_packet_with_policy(src, dst, buf, ancillary, BufferSealPolicy::AutoApplySeals)
}

/// Forwards data between non-blocking sockets while inspecting and enforcing buffer seal policy on SCM_RIGHTS file descriptors.
pub fn relay_packet_with_policy(
    src: RawFd,
    dst: RawFd,
    buf: &mut [u8],
    ancillary: &mut AncillaryFdBuffer,
    seal_policy: BufferSealPolicy,
) -> Result<RelayStatus, KuroganeError> {
    ancillary.clear();
    let num_bytes;

    {
        let mut iov = [IoSliceMut::new(buf)];
        let mut cmsg_space = nix::cmsg_space!([RawFd; 8]);

        let msg = match recvmsg::<()>(src, &mut iov, Some(&mut cmsg_space), MsgFlags::empty()) {
            Ok(m) => m,
            Err(e) if e == nix::errno::Errno::EAGAIN || e == nix::errno::Errno::EWOULDBLOCK => {
                return Ok(RelayStatus::WouldBlock);
            }
            Err(_) => return Err(KuroganeError::WaylandProxyError("Socket recvmsg error")),
        };

        num_bytes = msg.bytes;

        // Extract and inspect any received file descriptors (e.g. wl_shm pool or dmabuf)
        if let Ok(cmsgs) = msg.cmsgs() {
            for cmsg in cmsgs {
                if let ControlMessageOwned::ScmRights(fds) = cmsg {
                    for fd in fds {
                        // Enforce memory buffer seal policy
                        let _ = crate::wayland_wire::inspect_and_enforce_fd(fd, seal_policy)?;
                        let _ = ancillary.push(fd);
                    }
                }
            }
        }
    }

    if num_bytes == 0 {
        return Ok(RelayStatus::Eof);
    }

    // Forward payload and FDs to destination
    let out_iov = [IoSlice::new(&buf[..num_bytes])];
    let send_res = if ancillary.is_empty() {
        sendmsg::<()>(dst, &out_iov, &[], MsgFlags::empty(), None)
    } else {
        let cmsg = [ControlMessage::ScmRights(ancillary.as_slice())];
        sendmsg::<()>(dst, &out_iov, &cmsg, MsgFlags::empty(), None)
    };

    match send_res {
        Ok(_) => Ok(RelayStatus::Forwarded(num_bytes)),
        Err(e) if e == nix::errno::Errno::EAGAIN || e == nix::errno::Errno::EWOULDBLOCK => {
            Ok(RelayStatus::WouldBlock)
        }
        Err(_) => Err(KuroganeError::WaylandProxyError("Socket sendmsg error")),
    }
}

/// Runs the Wayland Switchboard Proxy in a single-threaded non-blocking epoll reactor on the current thread,
/// supporting arbitrarily many concurrent Wayland clients with egress buffering and dynamic global censorship.
pub fn run_standalone_reactor(
    proxy_socket: &str,
    compositor_socket: &str,
    _seal_policy: BufferSealPolicy,
) -> Result<(), KuroganeError> {
    let proxy_path = Path::new(proxy_socket);

    if let Some(parent) = proxy_path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_default();
    }

    if proxy_path.exists() {
        let _ = std::fs::remove_file(proxy_path);
    }

    let listener = UnixListener::bind(proxy_path)
        .map_err(|_| KuroganeError::WaylandProxyError("Failed to bind proxy socket"))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| KuroganeError::WaylandProxyError("Failed to set nonblocking on listener"))?;

    let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)
        .map_err(|_| KuroganeError::WaylandProxyError("Failed to create proxy epoll instance"))?;

    let borrowed_listener = unsafe { BorrowedFd::borrow_raw(listener.as_raw_fd()) };
    epoll
        .add(borrowed_listener, EpollEvent::new(EpollFlags::EPOLLIN, 1))
        .map_err(|_| KuroganeError::WaylandProxyError("Failed to register listener with epoll"))?;

    let comp_sock_path = compositor_socket.to_string();

    let mut sessions: std::collections::HashMap<u32, crate::wayland_engine::ClientSession> =
        std::collections::HashMap::new();
    let mut next_session_id = 1u32;
    let mut events = [EpollEvent::empty(); 16];

    const TOKEN_LISTENER: u64 = 1;
    const FLAG_CLIENT: u64 = 1 << 32;
    const FLAG_SERVER: u64 = 2 << 32;

    while let Ok(n) = epoll.wait(&mut events, EpollTimeout::from(100u16)) {
        let mut dead_sessions = Vec::new();

        for ev in events.iter().take(n) {
            let token = ev.data();

            if token == TOKEN_LISTENER {
                while let Ok((client_stream, _)) = listener.accept() {
                    let server_stream = match UnixStream::connect(&comp_sock_path) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };

                    let session_id = next_session_id;
                    next_session_id = next_session_id.wrapping_add(1);

                    let b_client = unsafe { BorrowedFd::borrow_raw(client_stream.as_raw_fd()) };
                    let b_server = unsafe { BorrowedFd::borrow_raw(server_stream.as_raw_fd()) };

                    let c_ev = EpollEvent::new(
                        EpollFlags::EPOLLIN
                            | EpollFlags::EPOLLOUT
                            | EpollFlags::EPOLLRDHUP
                            | EpollFlags::EPOLLHUP
                            | EpollFlags::EPOLLERR,
                        FLAG_CLIENT | (session_id as u64),
                    );
                    let s_ev = EpollEvent::new(
                        EpollFlags::EPOLLIN
                            | EpollFlags::EPOLLOUT
                            | EpollFlags::EPOLLRDHUP
                            | EpollFlags::EPOLLHUP
                            | EpollFlags::EPOLLERR,
                        FLAG_SERVER | (session_id as u64),
                    );

                    let _ = epoll.add(b_client, c_ev);
                    let _ = epoll.add(b_server, s_ev);

                    if let Ok(sess) =
                        crate::wayland_engine::ClientSession::new(client_stream, server_stream)
                    {
                        sessions.insert(session_id, sess);
                    }
                }
            } else if (token & FLAG_CLIENT) != 0 {
                let session_id = (token & 0xffff_ffff) as u32;
                if let Some(session) = sessions.get_mut(&session_id) {
                    if ev.events().contains(EpollFlags::EPOLLOUT) {
                        let _ = session.step_client_out();
                    }
                    if ev.events().contains(EpollFlags::EPOLLIN) {
                        match session.step_client_in() {
                            Ok(true) => {
                                let _ = session.step_server_out();
                            }
                            Ok(false) => {
                                dead_sessions.push(session_id);
                            }
                            Err(_) => {
                                dead_sessions.push(session_id);
                            }
                        }
                    }
                    if (ev.events().contains(EpollFlags::EPOLLRDHUP)
                        || ev.events().contains(EpollFlags::EPOLLHUP)
                        || ev.events().contains(EpollFlags::EPOLLERR))
                        && session.client_in_buf.is_empty()
                        && session.server_out_queue.is_empty()
                    {
                        dead_sessions.push(session_id);
                    }
                }
            } else if (token & FLAG_SERVER) != 0 {
                let session_id = (token & 0xffff_ffff) as u32;
                if let Some(session) = sessions.get_mut(&session_id) {
                    if ev.events().contains(EpollFlags::EPOLLOUT) {
                        let _ = session.step_server_out();
                    }
                    if ev.events().contains(EpollFlags::EPOLLIN) {
                        match session.step_server_in() {
                            Ok(true) => {
                                let _ = session.step_client_out();
                            }
                            Ok(false) => {
                                dead_sessions.push(session_id);
                            }
                            Err(_) => {
                                dead_sessions.push(session_id);
                            }
                        }
                    }
                    if (ev.events().contains(EpollFlags::EPOLLRDHUP)
                        || ev.events().contains(EpollFlags::EPOLLHUP)
                        || ev.events().contains(EpollFlags::EPOLLERR))
                        && session.server_in_buf.is_empty()
                        && session.client_out_queue.is_empty()
                    {
                        dead_sessions.push(session_id);
                    }
                }
            }
        }

        // Clean up disconnected sessions
        for sid in dead_sessions {
            if let Some(s) = sessions.remove(&sid) {
                let _ = epoll.delete(unsafe { BorrowedFd::borrow_raw(s.client.as_raw_fd()) });
                let _ = epoll.delete(unsafe { BorrowedFd::borrow_raw(s.server.as_raw_fd()) });
            }
        }
    }

    Ok(())
}

/// Starts the Wayland Switchboard Proxy (spawns single background worker for test/in-process callers).
pub fn start_proxy(proxy_socket: &str, compositor_socket: &str) -> Result<(), KuroganeError> {
    start_proxy_with_policy(
        proxy_socket,
        compositor_socket,
        BufferSealPolicy::AutoApplySeals,
    )
}

pub fn start_proxy_with_policy(
    proxy_socket: &str,
    compositor_socket: &str,
    seal_policy: BufferSealPolicy,
) -> Result<(), KuroganeError> {
    let proxy_str = proxy_socket.to_string();
    let comp_str = compositor_socket.to_string();

    thread::spawn(move || {
        let _ = run_standalone_reactor(&proxy_str, &comp_str, seal_policy);
    });

    Ok(())
}

/// Handles an active client session using a unified single-threaded epoll reactor.
pub fn handle_client_epoll(client: UnixStream, server: UnixStream) {
    handle_client_epoll_with_policy(client, server, BufferSealPolicy::AutoApplySeals);
}

/// Handles an active client session using a unified single-threaded epoll reactor with explicit seal policy.
pub fn handle_client_epoll_with_policy(
    client: UnixStream,
    server: UnixStream,
    seal_policy: BufferSealPolicy,
) {
    let client_fd = client.as_raw_fd();
    let server_fd = server.as_raw_fd();

    // Set sockets to non-blocking mode
    let _ = client.set_nonblocking(true);
    let _ = server.set_nonblocking(true);

    let epoll = match Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC) {
        Ok(e) => e,
        Err(_) => return,
    };

    let b_client = unsafe { BorrowedFd::borrow_raw(client_fd) };
    let b_server = unsafe { BorrowedFd::borrow_raw(server_fd) };

    let ev_client = EpollEvent::new(
        EpollFlags::EPOLLIN | EpollFlags::EPOLLHUP | EpollFlags::EPOLLRDHUP,
        1,
    );
    let ev_server = EpollEvent::new(
        EpollFlags::EPOLLIN | EpollFlags::EPOLLHUP | EpollFlags::EPOLLRDHUP,
        2,
    );

    let _ = epoll.add(b_client, ev_client);
    let _ = epoll.add(b_server, ev_server);

    let mut stream_buf = WireStreamBuffer::<8192>::new();
    let mut raw_buf = [0u8; 4096];
    let mut ancillary = AncillaryFdBuffer::<32>::new();
    let mut sealed = false;
    let mut processed_globals = 0;

    let mut events = [EpollEvent::empty(); 8];

    while let Ok(n) = epoll.wait(&mut events, EpollTimeout::from(5000u16)) {
        if n == 0 {
            continue;
        }

        for ev in events.iter().take(n) {
            let token = ev.data();

            // Client -> Server relay
            if token == 1 {
                match relay_packet_with_policy(
                    client_fd,
                    server_fd,
                    &mut raw_buf,
                    &mut ancillary,
                    seal_policy,
                ) {
                    Ok(RelayStatus::Eof) => return,
                    Ok(RelayStatus::WouldBlock) => {
                        if ev.events().contains(EpollFlags::EPOLLHUP)
                            || ev.events().contains(EpollFlags::EPOLLRDHUP)
                        {
                            return;
                        }
                    }
                    Ok(RelayStatus::Forwarded(_)) => {}
                    Err(_) => return,
                }
            }

            // Server -> Client relay with registry filtering
            if token == 2 {
                if !sealed {
                    // Handshake phase: inspect wl_registry globals
                    let dest = stream_buf.writable_slice_mut();
                    let bytes = {
                        let mut iov = [IoSliceMut::new(dest)];
                        let mut cmsg_space = nix::cmsg_space!([RawFd; 8]);
                        match recvmsg::<()>(
                            server_fd,
                            &mut iov,
                            Some(&mut cmsg_space),
                            MsgFlags::empty(),
                        ) {
                            Ok(m) if m.bytes == 0 => return,
                            Ok(m) => {
                                if let Ok(cmsgs) = m.cmsgs() {
                                    for cmsg in cmsgs {
                                        if let ControlMessageOwned::ScmRights(fds) = cmsg {
                                            for fd in fds {
                                                let _ = crate::wayland_wire::inspect_and_enforce_fd(
                                                    fd,
                                                    seal_policy,
                                                );
                                                let _ = ancillary.push(fd);
                                            }
                                        }
                                    }
                                }
                                m.bytes
                            }
                            Err(e)
                                if e == nix::errno::Errno::EAGAIN
                                    || e == nix::errno::Errno::EWOULDBLOCK =>
                            {
                                0
                            }
                            Err(_) => return,
                        }
                    };

                    if bytes > 0 {
                        stream_buf.commit_write(bytes);

                        let mut forwarded_any = false;
                        while let Some(msg) = stream_buf.next_message() {
                            forwarded_any = true;
                            let mut forward_msg = true;

                            if let Some(global) = RegistryGlobal::parse(msg.payload) {
                                if is_blacklisted_interface(global.interface) {
                                    println!(
                                        "\x1b[1;33m[kurogane-proxy]\x1b[0m CENSORED restricted Wayland interface: '{}'",
                                        global.interface
                                    );
                                    forward_msg = false;
                                }
                                processed_globals += 1;
                            }

                            if forward_msg {
                                let mut hdr_bytes = [0u8; 8];
                                msg.header.serialize(&mut hdr_bytes);
                                let out_iov = [IoSlice::new(&hdr_bytes), IoSlice::new(msg.payload)];
                                if !ancillary.is_empty() {
                                    let cmsg = [ControlMessage::ScmRights(ancillary.as_slice())];
                                    if let Err(e) = sendmsg::<()>(
                                        client_fd,
                                        &out_iov,
                                        &cmsg,
                                        MsgFlags::empty(),
                                        None,
                                    ) {
                                        eprintln!("[proxy] sendmsg error with FDs: {:?}", e);
                                    }
                                    ancillary.clear();
                                } else {
                                    if let Err(e) = sendmsg::<()>(
                                        client_fd,
                                        &out_iov,
                                        &[],
                                        MsgFlags::empty(),
                                        None,
                                    ) {
                                        eprintln!("[proxy] sendmsg error: {:?}", e);
                                    }
                                }
                            }

                            if processed_globals >= 1 && stream_buf.available() == 0 {
                                sealed = true;
                                break;
                            }
                        }

                        if !forwarded_any && stream_buf.available() > 0 {
                            let unconsumed = stream_buf.next_raw_slice();
                            let out_iov = [IoSlice::new(unconsumed)];
                            let _ =
                                sendmsg::<()>(client_fd, &out_iov, &[], MsgFlags::empty(), None);
                            sealed = true;
                        }
                    }
                } else {
                    // Fast path after handshake sealed: forward packets & ancillary fds
                    match relay_packet_with_policy(
                        server_fd,
                        client_fd,
                        &mut raw_buf,
                        &mut ancillary,
                        seal_policy,
                    ) {
                        Ok(RelayStatus::Eof) => return,
                        Ok(RelayStatus::WouldBlock) => {}
                        Ok(RelayStatus::Forwarded(_)) => {}
                        Err(_) => return,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland_wire::WaylandHeader;
    use std::io::{Read, Write};
    use std::time::Duration;

    #[test]
    fn test_proxy_data_relay() {
        let temp_dir = std::env::temp_dir();
        let proxy_sock = temp_dir.join("test_kurogane_wayland_proxy.sock");
        let comp_sock = temp_dir.join("test_kurogane_wayland_comp.sock");

        let _ = std::fs::remove_file(&proxy_sock);
        let _ = std::fs::remove_file(&comp_sock);

        let comp_listener = UnixListener::bind(&comp_sock).expect("bind fake compositor");

        start_proxy(proxy_sock.to_str().unwrap(), comp_sock.to_str().unwrap())
            .expect("start proxy");

        let comp_handle = thread::spawn(move || {
            let (mut stream, _) = comp_listener.accept().expect("comp accept");
            let mut buf = [0u8; 16];
            stream.read_exact(&mut buf).expect("comp read");
            assert_eq!(&buf[8..16], b"HELLO_WA");

            let mut msg = [0u8; 16];
            let header = WaylandHeader {
                object_id: 1,
                opcode: 0,
                size: 16,
            };
            let mut hdr_bytes = [0u8; 8];
            header.serialize(&mut hdr_bytes);
            msg[..8].copy_from_slice(&hdr_bytes);
            msg[8..16].copy_from_slice(b"PONG_WAY");
            stream.write_all(&msg).expect("comp write");
        });

        thread::sleep(Duration::from_millis(50));

        let mut client = UnixStream::connect(&proxy_sock).expect("client connect to proxy");
        let mut client_msg = [0u8; 16];
        let header = WaylandHeader {
            object_id: 1,
            opcode: 0,
            size: 16,
        };
        let mut hdr_bytes = [0u8; 8];
        header.serialize(&mut hdr_bytes);
        client_msg[..8].copy_from_slice(&hdr_bytes);
        client_msg[8..16].copy_from_slice(b"HELLO_WA");
        client.write_all(&client_msg).expect("client write");

        let mut reply = [0u8; 16];
        client.read_exact(&mut reply).expect("client read");
        assert_eq!(&reply[8..16], b"PONG_WAY");

        comp_handle.join().expect("join comp thread");

        let _ = std::fs::remove_file(&proxy_sock);
        let _ = std::fs::remove_file(&comp_sock);
    }

    #[test]
    fn test_active_registry_global_filtering() {
        let temp_dir = std::env::temp_dir();
        let proxy_sock = temp_dir.join("test_kurogane_filter_proxy.sock");
        let comp_sock = temp_dir.join("test_kurogane_filter_comp.sock");

        let _ = std::fs::remove_file(&proxy_sock);
        let _ = std::fs::remove_file(&comp_sock);

        let comp_listener = UnixListener::bind(&comp_sock).expect("bind fake compositor");

        start_proxy(proxy_sock.to_str().unwrap(), comp_sock.to_str().unwrap())
            .expect("start filter proxy");

        // Compositor thread: emits 2 globals: wl_compositor (allowed) and wlr_screencopy_v1 (blacklisted)
        let comp_handle = thread::spawn(move || {
            let (mut stream, _) = comp_listener.accept().expect("comp accept");

            // 1. Emit allowed: wl_compositor
            let mut msg1 = [0u8; 48];
            let len1 =
                RegistryGlobal::serialize_message(2, 1, "wl_compositor", 4, &mut msg1).unwrap();
            stream.write_all(&msg1[..len1]).expect("write msg1");

            // 2. Emit blacklisted: wlr_screencopy_v1
            let mut msg2 = [0u8; 48];
            let len2 =
                RegistryGlobal::serialize_message(2, 2, "wlr_screencopy_v1", 1, &mut msg2).unwrap();
            stream.write_all(&msg2[..len2]).expect("write msg2");
        });

        thread::sleep(Duration::from_millis(50));

        let mut client = UnixStream::connect(&proxy_sock).expect("client connect to filter proxy");
        let mut received = [0u8; 128];
        let bytes_read = client
            .read(&mut received)
            .expect("read sanitized client stream");

        // Parse received messages on client side
        let header1 = WaylandHeader::parse(&received[..bytes_read]).expect("parse header 1");
        let global1 =
            RegistryGlobal::parse(&received[8..header1.size as usize]).expect("parse global 1");
        assert_eq!(global1.interface, "wl_compositor");

        // Assert that client received ONLY 1 message (wl_compositor), and wlr_screencopy_v1 was dropped!
        assert_eq!(bytes_read, header1.size as usize);

        comp_handle.join().expect("join comp thread");

        let _ = std::fs::remove_file(&proxy_sock);
        let _ = std::fs::remove_file(&comp_sock);
    }

    #[test]
    fn test_proxy_multi_client_concurrency() {
        let temp_dir = std::env::temp_dir();
        let proxy_sock = temp_dir.join("test_kurogane_multi_proxy.sock");
        let comp_sock = temp_dir.join("test_kurogane_multi_comp.sock");

        let _ = std::fs::remove_file(&proxy_sock);
        let _ = std::fs::remove_file(&comp_sock);

        let comp_listener =
            UnixListener::bind(&comp_sock).expect("bind fake comp for multi client");

        start_proxy(proxy_sock.to_str().unwrap(), comp_sock.to_str().unwrap())
            .expect("start multi proxy");

        // Compositor thread handles 2 connections concurrently
        let comp_handle = thread::spawn(move || {
            let (mut s1, _) = comp_listener.accept().expect("comp accept conn 1");
            let (mut s2, _) = comp_listener.accept().expect("comp accept conn 2");

            let mut b1 = [0u8; 16];
            s1.read_exact(&mut b1).expect("s1 read");
            assert_eq!(&b1[8..16], b"CLIENT_1");

            let mut b2 = [0u8; 16];
            s2.read_exact(&mut b2).expect("s2 read");
            assert_eq!(&b2[8..16], b"CLIENT_2");

            // Reply to both with valid 16-byte frames
            let mut r1 = [0u8; 16];
            let mut h1 = [0u8; 8];
            WaylandHeader {
                object_id: 1,
                opcode: 0,
                size: 16,
            }
            .serialize(&mut h1);
            r1[..8].copy_from_slice(&h1);
            r1[8..16].copy_from_slice(b"REPLY_1_");
            s1.write_all(&r1).expect("s1 write");

            let mut r2 = [0u8; 16];
            let mut h2 = [0u8; 8];
            WaylandHeader {
                object_id: 1,
                opcode: 0,
                size: 16,
            }
            .serialize(&mut h2);
            r2[..8].copy_from_slice(&h2);
            r2[8..16].copy_from_slice(b"REPLY_2_");
            s2.write_all(&r2).expect("s2 write");
        });

        thread::sleep(Duration::from_millis(50));

        // Connect Client 1 and Client 2 simultaneously
        let mut c1 = UnixStream::connect(&proxy_sock).expect("c1 connect");
        let mut c2 = UnixStream::connect(&proxy_sock).expect("c2 connect");

        let mut m1 = [0u8; 16];
        let mut hm1 = [0u8; 8];
        WaylandHeader {
            object_id: 1,
            opcode: 0,
            size: 16,
        }
        .serialize(&mut hm1);
        m1[..8].copy_from_slice(&hm1);
        m1[8..16].copy_from_slice(b"CLIENT_1");

        let mut m2 = [0u8; 16];
        let mut hm2 = [0u8; 8];
        WaylandHeader {
            object_id: 1,
            opcode: 0,
            size: 16,
        }
        .serialize(&mut hm2);
        m2[..8].copy_from_slice(&hm2);
        m2[8..16].copy_from_slice(b"CLIENT_2");

        // Both write without either closing
        c1.write_all(&m1).expect("c1 write");
        c2.write_all(&m2).expect("c2 write");

        // Both read their respective replies concurrently
        let mut rep1 = [0u8; 16];
        let mut rep2 = [0u8; 16];
        c1.read_exact(&mut rep1).expect("c1 read");
        c2.read_exact(&mut rep2).expect("c2 read");

        assert_eq!(&rep1[8..16], b"REPLY_1_");
        assert_eq!(&rep2[8..16], b"REPLY_2_");

        comp_handle.join().expect("comp thread join");

        let _ = std::fs::remove_file(&proxy_sock);
        let _ = std::fs::remove_file(&comp_sock);
    }
}

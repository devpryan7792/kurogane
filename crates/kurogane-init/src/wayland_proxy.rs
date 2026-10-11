use crate::wayland_wire::{
    is_blacklisted_interface, AncillaryFdBuffer, RegistryGlobal, WireStreamBuffer,
};
use crate::KuroganeError;
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags, EpollTimeout};
use nix::sys::socket::{
    recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags,
};
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

/// Forwards data between non-blocking sockets with SCM_RIGHTS file descriptor preservation.
pub fn relay_packet(
    src: RawFd,
    dst: RawFd,
    buf: &mut [u8],
    ancillary: &mut AncillaryFdBuffer,
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

        // Extract any received file descriptors (e.g. wl_shm pool or dmabuf)
        if let Ok(cmsgs) = msg.cmsgs() {
            for cmsg in cmsgs {
                if let ControlMessageOwned::ScmRights(fds) = cmsg {
                    let _ = ancillary.extend_from_slice(&fds);
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

/// Starts the Wayland Switchboard Proxy in a single non-blocking event-driven loop.
pub fn start_proxy(proxy_socket: &str, compositor_socket: &str) -> Result<(), KuroganeError> {
    let proxy_path = Path::new(proxy_socket);

    // Ensure the parent directory exists
    if let Some(parent) = proxy_path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_default();
    }

    if proxy_path.exists() {
        let _ = std::fs::remove_file(proxy_path);
    }

    let listener = UnixListener::bind(proxy_path)
        .map_err(|_| KuroganeError::WaylandProxyError("Failed to bind proxy socket"))?;

    let comp_sock_path = compositor_socket.to_string();

    // Spawn background listener thread for the proxy server
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(client_stream) => {
                    if let Ok(server_stream) = UnixStream::connect(&comp_sock_path) {
                        thread::spawn(move || {
                            handle_client_epoll(client_stream, server_stream);
                        });
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(())
}

/// Handles an active client session using a unified single-threaded epoll reactor.
/// Intercepts restricted registry globals during handshake and forwards SCM_RIGHTS fds.
pub fn handle_client_epoll(client: UnixStream, server: UnixStream) {
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

    let ev_client = EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLHUP | EpollFlags::EPOLLRDHUP, 1);
    let ev_server = EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLHUP | EpollFlags::EPOLLRDHUP, 2);

    let _ = epoll.add(b_client, ev_client);
    let _ = epoll.add(b_server, ev_server);

    let mut stream_buf = WireStreamBuffer::<8192>::new();
    let mut raw_buf = [0u8; 4096];
    let mut ancillary = AncillaryFdBuffer::<32>::new();
    let mut sealed = false;
    let mut processed_globals = 0;

    let mut events = [EpollEvent::empty(); 8];

    loop {
        let n = match epoll.wait(&mut events, EpollTimeout::from(5000u16)) {
            Ok(n) => n,
            Err(_) => break,
        };

        if n == 0 {
            continue;
        }

        for ev in events.iter().take(n) {
            let token = ev.data();

            // Client -> Server relay
            if token == 1 {
                match relay_packet(client_fd, server_fd, &mut raw_buf, &mut ancillary) {
                    Ok(RelayStatus::Eof) => return,
                    Ok(RelayStatus::WouldBlock) => {
                        if ev.events().contains(EpollFlags::EPOLLHUP) || ev.events().contains(EpollFlags::EPOLLRDHUP) {
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
                        match recvmsg::<()>(server_fd, &mut iov, Some(&mut cmsg_space), MsgFlags::empty()) {
                            Ok(m) if m.bytes == 0 => return,
                            Ok(m) => {
                                if let Ok(cmsgs) = m.cmsgs() {
                                    for cmsg in cmsgs {
                                        if let ControlMessageOwned::ScmRights(fds) = cmsg {
                                            let _ = ancillary.extend_from_slice(&fds);
                                        }
                                    }
                                }
                                m.bytes
                            }
                            Err(e) if e == nix::errno::Errno::EAGAIN || e == nix::errno::Errno::EWOULDBLOCK => 0,
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
                                    if let Err(e) = sendmsg::<()>(client_fd, &out_iov, &cmsg, MsgFlags::empty(), None) {
                                        eprintln!("[proxy] sendmsg error with FDs: {:?}", e);
                                    }
                                    ancillary.clear();
                                } else {
                                    if let Err(e) = sendmsg::<()>(client_fd, &out_iov, &[], MsgFlags::empty(), None) {
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
                            let _ = sendmsg::<()>(client_fd, &out_iov, &[], MsgFlags::empty(), None);
                            sealed = true;
                        }
                    }
                } else {
                    // Fast path after handshake sealed: forward packets & ancillary fds
                    match relay_packet(server_fd, client_fd, &mut raw_buf, &mut ancillary) {
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
            let mut buf = [0u8; 13];
            stream.read_exact(&mut buf).expect("comp read");
            assert_eq!(&buf, b"HELLO_WAYLAND");

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
        client.write_all(b"HELLO_WAYLAND").expect("client write");

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
}

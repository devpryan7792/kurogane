use kurogane_init::wayland_engine::{ClientSession, OwnedFd};
use std::collections::VecDeque;

#[test]
fn prove_wayland_fd_desynchronization_bug_resolved() {
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

    // Simulate socket delivering both frames + 1 FD in single recvmsg
    let mut in_buf = Vec::new();
    in_buf.extend_from_slice(&frame1_bytes);
    in_buf.extend_from_slice(&frame2_bytes);

    // Create a real OS pipe descriptor representing the keymap
    let mut pipe_fds = [-1; 2];
    unsafe {
        libc::pipe(pipe_fds.as_mut_ptr());
    }
    let mut in_fds = VecDeque::new();
    in_fds.push_back(OwnedFd::new(pipe_fds[0]));
    unsafe {
        libc::close(pipe_fds[1]);
    }

    // Run the extraction function with schema-aware resolver
    let extracted = ClientSession::extract_frames(&mut in_buf, &mut in_fds, |id, op| {
        if id == 4 && op == 0 {
            1
        } else {
            0
        }
    });

    assert_eq!(extracted.len(), 2);

    println!(
        "Frame 1 (Surface Enter) received {} FDs",
        extracted[0].fds.len()
    );
    println!(
        "Frame 2 (Keyboard Keymap) received {} FDs",
        extracted[1].fds.len()
    );

    // Invariant: Frame 1 must NOT steal the FD!
    assert_eq!(
        extracted[0].fds.len(),
        0,
        "wl_surface.enter must NOT steal the keyboard keymap file descriptor!"
    );
    // Invariant: Frame 2 must receive its exact descriptor!
    assert_eq!(
        extracted[1].fds.len(),
        1,
        "wl_keyboard.keymap must receive its exact file descriptor!"
    );
}

#[test]
fn prove_unmodeled_extension_fd_does_not_desync_keyboard_keymap() {
    use kurogane_init::wayland_engine::WaylandInterface;
    use std::collections::HashMap;

    // Interface map: ID 4 is Keyboard, ID 99 is completely UNMODELED extension
    let mut interfaces = HashMap::new();
    interfaces.insert(4, WaylandInterface::Keyboard);

    // Frame 1: Unmodeled extension (Object ID 99, Opcode 0, Size 8)
    let frame1_bytes = [
        99, 0, 0, 0, // object_id = 99 (unknown extension)
        0, 0, // opcode = 0
        8, 0, // size = 8 bytes
    ];

    // Frame 2: wl_keyboard.keymap (Object ID 4, Opcode 0, Size 12)
    let frame2_bytes = [
        4, 0, 0, 0, // object_id = 4 (Keyboard)
        0, 0, // opcode = 0
        12, 0, // size = 12 bytes
        1, 0, 0, 0, // format = 1
    ];

    let mut in_buf = Vec::new();
    in_buf.extend_from_slice(&frame1_bytes);
    in_buf.extend_from_slice(&frame2_bytes);

    // Two distinct OS pipes: Pipe 1 for unmodeled extension, Pipe 2 for keymap
    let mut pipe1 = [-1; 2];
    let mut pipe2 = [-1; 2];
    unsafe {
        libc::pipe(pipe1.as_mut_ptr());
        libc::pipe(pipe2.as_mut_ptr());
    }

    let mut in_fds = VecDeque::new();
    in_fds.push_back(OwnedFd::new(pipe1[0]));
    in_fds.push_back(OwnedFd::new(pipe2[0]));

    unsafe {
        libc::close(pipe1[1]);
        libc::close(pipe2[1]);
    }

    let p1_read_fd = pipe1[0];
    let p2_read_fd = pipe2[0];

    // Extract frames using the live resolve_expected_fds implementation
    let extracted = ClientSession::extract_frames(&mut in_buf, &mut in_fds, |id, op| {
        ClientSession::resolve_expected_fds(&interfaces, id, op, true)
    });

    assert_eq!(extracted.len(), 2);

    // Frame 1 (unmodeled) must dynamically consume its queued FD rather than stranding it
    assert_eq!(
        extracted[0].fds.len(),
        1,
        "Unmodeled extension must consume its attached FD"
    );
    assert_eq!(
        extracted[0].fds[0].as_raw_fd(),
        p1_read_fd,
        "Unmodeled extension must receive Pipe 1 FD"
    );

    // Frame 2 (keymap) must receive its exact keymap FD, NOT the stale unmodeled FD!
    assert_eq!(
        extracted[1].fds.len(),
        1,
        "Keyboard keymap must receive its attached FD"
    );
    assert_eq!(
        extracted[1].fds[0].as_raw_fd(),
        p2_read_fd,
        "Keyboard keymap must receive Pipe 2 FD (NO PHASE SHIFT)"
    );

    // Invariant: Zero stranded FDs left in queue
    assert_eq!(
        in_fds.len(),
        0,
        "No file descriptors must remain stranded in the queue"
    );
}

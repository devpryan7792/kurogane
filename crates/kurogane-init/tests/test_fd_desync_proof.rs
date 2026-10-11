use kurogane_init::wayland_engine::{ClientSession, OwnedFd};
use std::collections::VecDeque;

#[test]
fn prove_wayland_fd_desynchronization_bug_resolved() {
    // Frame 1: wl_surface.enter (Object ID 3, Opcode 0, Size 8) -> Takes 0 FDs
    let frame1_bytes = [
        3, 0, 0, 0,  // object_id = 3
        0, 0,        // opcode = 0
        8, 0,        // size = 8 bytes
    ];

    // Frame 2: wl_keyboard.keymap (Object ID 4, Opcode 0, Size 12) -> Takes 1 FD
    let frame2_bytes = [
        4, 0, 0, 0,  // object_id = 4
        0, 0,        // opcode = 0
        12, 0,       // size = 12 bytes
        1, 0, 0, 0,  // format = 1
    ];

    // Simulate socket delivering both frames + 1 FD in single recvmsg
    let mut in_buf = Vec::new();
    in_buf.extend_from_slice(&frame1_bytes);
    in_buf.extend_from_slice(&frame2_bytes);

    // Create a real OS pipe descriptor representing the keymap
    let mut pipe_fds = [-1; 2];
    unsafe { libc::pipe(pipe_fds.as_mut_ptr()); }
    let mut in_fds = VecDeque::new();
    in_fds.push_back(OwnedFd::new(pipe_fds[0]));
    unsafe { libc::close(pipe_fds[1]); }

    // Run the extraction function with schema-aware resolver
    let extracted = ClientSession::extract_frames(
        &mut in_buf,
        &mut in_fds,
        |id, op| {
            if id == 4 && op == 0 { 1 } else { 0 }
        },
    );

    assert_eq!(extracted.len(), 2);

    println!("Frame 1 (Surface Enter) received {} FDs", extracted[0].fds.len());
    println!("Frame 2 (Keyboard Keymap) received {} FDs", extracted[1].fds.len());

    // Invariant: Frame 1 must NOT steal the FD!
    assert_eq!(
        extracted[0].fds.len(), 0,
        "wl_surface.enter must NOT steal the keyboard keymap file descriptor!"
    );
    // Invariant: Frame 2 must receive its exact descriptor!
    assert_eq!(
        extracted[1].fds.len(), 1,
        "wl_keyboard.keymap must receive its exact file descriptor!"
    );
}

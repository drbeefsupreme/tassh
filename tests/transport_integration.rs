//! Integration tests for the TCP transport layer.
//!
//! These tests exercise `send_frame` / `recv_frame` directly over a loopback
//! TCP connection, without going through the higher-level `server()` / `client()`
//! functions that introduce reconnect loops and long-lived tasks.

use tassh::protocol::{Frame, FRAME_TYPE_PNG};
use tassh::transport::{recv_frame, send_frame};
use tokio::net::{TcpListener, TcpStream};

/// Verify that a PNG frame sent over a TCP loopback connection arrives with
/// byte-perfect fidelity — both `frame_type` and `payload` must match.
///
/// This validates XPRT-01 (TCP connection works) and the frame integrity
/// requirement from the plan success criteria.
#[tokio::test]
async fn test_frame_traversal_loopback() {
    // Bind on an OS-assigned port to avoid conflicts.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let payload = vec![0x89u8, 0x50, 0x4E, 0x47]; // PNG magic bytes

    // Spawn server side: accept one connection, receive one frame, return it.
    let (oneshot_tx, oneshot_rx) = tokio::sync::oneshot::channel::<Frame>();
    tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.unwrap();
        let (mut reader, _writer) = stream.into_split();
        let frame = recv_frame(&mut reader).await.unwrap();
        oneshot_tx.send(frame).unwrap();
    });

    // Client side: connect and send one frame.
    let client_stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (_reader, mut writer) = client_stream.into_split();
    let frame_out = Frame::new_png(payload.clone());
    send_frame(&mut writer, &frame_out).await.unwrap();

    // Wait for the server to echo it back through the oneshot channel.
    let frame_in = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        oneshot_rx,
    )
    .await
    .expect("timed out waiting for frame")
    .expect("oneshot channel closed");

    assert_eq!(frame_in.frame_type, FRAME_TYPE_PNG);
    assert_eq!(frame_in.payload, payload);
}

/// Verify that after the server drops a connection the client can reconnect
/// on the same port and successfully transfer a frame.
///
/// This validates XPRT-03 (reconnect after server restart).
#[tokio::test]
async fn test_reconnect_after_server_restart() {
    // First server binding: accept connection, then drop it (simulating a crash).
    let listener1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener1.local_addr().unwrap().port();

    // Spawn first server — accept and immediately drop the connection.
    let first_done = tokio::spawn(async move {
        let (_stream, _peer) = listener1.accept().await.unwrap();
        // Drop stream here, simulating a server crash.
    });

    // Client connects to the first server.
    let client1 = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    // Wait for the first server to drop the connection.
    first_done.await.unwrap();
    // Drop client-side connection to clean up.
    drop(client1);

    // Short delay to let the OS release the port.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Second server binding on the same port.
    let listener2 = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    let payload = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let (oneshot_tx, oneshot_rx) = tokio::sync::oneshot::channel::<Frame>();

    // Spawn second server — accept one connection, receive one frame.
    tokio::spawn(async move {
        let (stream, _peer) = listener2.accept().await.unwrap();
        let (mut reader, _writer) = stream.into_split();
        let frame = recv_frame(&mut reader).await.unwrap();
        oneshot_tx.send(frame).unwrap();
    });

    // New client connects to the second server and sends a frame.
    let client2 = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        TcpStream::connect(format!("127.0.0.1:{port}")),
    )
    .await
    .expect("timed out reconnecting")
    .expect("reconnect failed");

    let (_reader, mut writer) = client2.into_split();
    send_frame(&mut writer, &Frame::new_png(payload.clone()))
        .await
        .unwrap();

    // Verify the frame arrives intact.
    let frame_in = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        oneshot_rx,
    )
    .await
    .expect("timed out waiting for frame after reconnect")
    .expect("oneshot channel closed");

    assert_eq!(frame_in.frame_type, FRAME_TYPE_PNG);
    assert_eq!(frame_in.payload, payload);
}

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the client that sent a request is still connected. Requests from a
/// client that left while it was only waiting are dropped, not served.
#[derive(Clone, Default)]
pub(crate) struct ClientPresence(Option<Arc<AtomicBool>>);

impl ClientPresence {
    pub(crate) fn watched() -> (Self, Arc<AtomicBool>) {
        let gone = Arc::new(AtomicBool::new(false));
        (Self(Some(Arc::clone(&gone))), gone)
    }

    pub(crate) fn departed(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|gone| gone.load(Ordering::SeqCst))
    }
}

impl PartialEq for ClientPresence {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for ClientPresence {}

impl std::fmt::Debug for ClientPresence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClientPresence(departed={})", self.departed())
    }
}

/// The client sends one frame and then only reads, so an orderly EOF or a reset
/// on a non-blocking peek means it has gone.
pub(crate) fn peer_hung_up(stream: &UnixStream) -> bool {
    let mut byte = 0u8;
    let n = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&raw mut byte).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    if n == 0 {
        return true;
    }
    n < 0 && io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock
}

pub(crate) fn client_left(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_hang_up_is_seen_only_after_close() {
        let (client, server) = UnixStream::pair().unwrap();
        assert!(!peer_hung_up(&server));
        drop(client);
        assert!(peer_hung_up(&server));
    }

    #[test]
    fn unwatched_presence_never_departs() {
        assert!(!ClientPresence::default().departed());
        let (presence, gone) = ClientPresence::watched();
        assert!(!presence.departed());
        gone.store(true, Ordering::SeqCst);
        assert!(presence.departed());
    }
}

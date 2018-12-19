use crate::error::{ApplicationError, ProtoError};

use failure::Fail;
use std::io::{self, Read, Write};
use std::net::ToSocketAddrs;
use udt::{SocketFamily, SocketType, UdtSocket};

pub const BLOCK_SIZE: usize = 128 * 1024;

enum Message {
	ReqIV,
	RepIV,
	Hello { nonce: u8 },
	Goodbye,
}

enum Mode {
	Sender,
	Receiver,
}

enum State {
	WaitHangup,
	WaitHello,
	Transmit,
}

struct Stream {
	inner: UdtSocket,
}

/// The `Stream` represents an underlying UDT socket.
impl Stream {
	/// When created in the `Receiver` mode it begins listening on the
	/// specified address. Otherwise if created in `Sender` mode it attempts
	/// to reach a receiver at the specified remote address.
	pub fn new<S: ToSocketAddrs>(mode: Mode, addr: S) -> Result<Self, ProtoError> {
		let sock_addr = addr.to_socket_addrs()?
			.take(1).next()
			.expect("fatal: expected a socket address but did not get one.");

		let stream = match mode {
			Mode::Sender => Self::create_sender()?,
			Mode::Receiver => Self::create_receiver()?,
		};

		Ok(stream)
	}


	fn create_sender() -> Result<Self, ProtoError> {
		unreachable!()
	}

	fn create_receiver() -> Result<Self, ProtoError> {
		unreachable!()
	}
}

impl Read for Stream {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
		let buf_len = buf.len();
		let bytes_recvd = self.inner.recv(buf, buf_len)
			.map_err(|err| ApplicationError::SocketErr { inner: err }.compat())
			.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;

		// TODO: check the sanity of this cast.
		//       not sure why UDT has this as a signed integer.
		Ok(bytes_recvd as usize)
	}
}

impl Write for Stream {
	fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
		let bytes_sent = self.inner.send(&buf)
			.map_err(|err| ApplicationError::SocketErr { inner: err }.compat())
			.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;

		// TODO: check the sanity of this cast.
		//       not sure why UDT has this as a signed integer.
		Ok(bytes_sent as usize)
	}

	fn flush(&mut self) -> Result<(), io::Error> {
		// TODO: UDT bindings provides no means to flush, I believe it's buffering
		// data internally and sending as fast as it can. (See: UDT_LINGER.)
		// for now this is a no-op since data is immediately committed to the
		// underlying UDT socket. 
		Ok(())
	}
}

/// A `Stream` implements both halves of the `ubuffer` protocol.
///
/// If a stream is created in the `Receiver` mode it will create a
/// UDT socket and begin listening on the specified address. When a
/// corresponding `Sender` connects to that same address the receiver
/// begins the handshake process which works as follows: 
///
/// 1. The receiver sends a `ReqIV` to the sender.
/// 2. The sender replies with a randomly created seed in a `RepIV` message.
/// 3. The receiver and sender both initialize their ciphers using their keys
///    (configured out of band) as well as the agreed upon IV.
/// 4. The receiver sends an encrypted `Hello` message with a nonce.
/// 5. The sender acknowledges this by encrypting its own `Hello` message
///    with a corresponding nonce.
/// 6. Both streams enter the `Transmit` state and begin exchanging encrypted
///    blocks with each other.
///
pub struct Sender {

}



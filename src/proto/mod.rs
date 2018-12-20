pub use self::receiver::Receiver;
pub use self::sender::Sender;

use crate::error::ProtoError;
use failure::Fail;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use udt::{SocketFamily, SocketType, UdtSocket};

mod receiver;
mod sender;
mod util;

pub const BLOCK_SIZE: usize = 128 * 1024;
pub const MAGIC_BYTES: u32 = 0xDEADBEEF;
pub const MESSAGE_SIZE: usize = 12;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
enum MessageTy {
	/// The data which follows is an incoming block of data from the sender.
	/// The `len` bytes which follow this message are encrypted with the 
	/// parameters agreed upon at the beginning of the session.
	Block,

	/// The sender is informing the receiver that it would like initialization
	/// parameters for the session's encryption. The sender will wait for four
	/// bytes (32-bits) which will be prepended to a 64-bit counter for each 
	/// message sent.
	ReqIV,

	/// The receiver chooses encryption parameters for the session and sends
	/// them as the following four bytes.
	RepIV,

	/// The sender acknowledges receipt of the nonce with an encrypted `Hello`.
	Hello,

	/// The sender informs the receiver that it is done sending blocks with
	/// a `Goodbye` message.
	Goodbye,
}

#[derive(Debug, Deserialize, Serialize)]
struct Message {
	ty: MessageTy,
	len: usize
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
			Mode::Sender => Self::create_sender(sock_addr)?,
			Mode::Receiver => Self::create_receiver(sock_addr)?,
		};

		Ok(stream)
	}


	fn create_sender(addr: SocketAddr) -> Result<Self, ProtoError> {
		info!("connecting to utp receiver ...");
		let sock = UdtSocket::new(SocketFamily::AFInet, SocketType::Stream)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		sock.connect(addr)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		Ok(Self { inner: sock })
	}

	fn create_receiver(addr: SocketAddr) -> Result<Self, ProtoError> {
		info!("setting up receiver socket ...");
		let sock = UdtSocket::new(SocketFamily::AFInet, SocketType::Stream)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		sock.bind(addr)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		sock.listen(1)
			.map_err(|err| ProtoError::SocketErr { inner: err })?;

		let (sock, _addr) = sock.accept()?;

		Ok(Self { inner: sock })
	}

	fn as_socket(&self) -> &UdtSocket { &self.inner }
}

impl Read for Stream {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
		let buf_len = buf.len();
		let bytes_recvd = self.inner.recv(buf, buf_len)
			.map_err(|err| ProtoError::SocketErr { inner: err }.compat())
			.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;

		// TODO: check the sanity of this cast.
		//       not sure why UDT has this as a signed integer.
		Ok(bytes_recvd as usize)
	}
}

impl Write for Stream {
	fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
		let bytes_sent = self.inner.send(&buf)
			.map_err(|err| ProtoError::SocketErr { inner: err }.compat())
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
